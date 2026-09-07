//! PROBLEM 245 — the in-app updater. Spaceadom installs its own updates.
//!
//! Owner's decisions, all final: check once at launch (after the autostart
//! settle) and then daily; install SILENTLY with no prompt; everyone gets a
//! release at the same moment, because GitHub Releases is the only channel;
//! both installers keep shipping. Everything below follows from those.
//!
//! **THE ONE RULE THAT MATTERS: an install must only ever be updated by the
//! SAME KIND of installer that made it.** A per-user `setup.exe` (NSIS) copy
//! updated with the `.msi` produces PROBLEM 129 (two copies, both grabbing the
//! spacebar) or PROBLEM 244 (the `.msi` adopts the live folder and a later
//! uninstall deletes the running app). So this module decides, at runtime and
//! from evidence on disk, how THIS copy was installed, and points the updater
//! at the manifest for that kind:
//!
//! | kind | evidence | manifest |
//! | --- | --- | --- |
//! | NSIS | `uninstall.exe` beside `spaceadom.exe` — NSIS writes it, WiX never does | `latest.json` |
//! | MSI | no `uninstall.exe`, and an HKLM Uninstall entry named Spaceadom whose `UninstallString` is `MsiExec /X…` and whose `InstallLocation` IS this exe's folder | `latest-msi.json` |
//! | Unknown | neither (a `target\release` dev build, a copied folder) | no check at all |
//!
//! `uninstall.exe` wins over an MSI registration on purpose: PROBLEM 244's
//! shape is exactly "an MSI product registered for the NSIS folder", and in
//! that state the FILES were put there by NSIS, so NSIS is what may replace
//! them. The decision is a pure function (`classify_install`) with tests.
//!
//! **THE MSI LEG IS ON (`MSI_AUTO_UPDATE = true`, PROBLEM 246).** The owner's
//! decision, 2026-09-05: an MSI-installed copy updates itself too, and the
//! user sees exactly ONE UAC prompt and nothing else. That prompt is not a
//! choice we make; it is the price of a per-machine package. The trade is
//! forced by Windows Installer and was measured, not assumed:
//!
//! | UI level | what a NON-elevated msiexec does to a per-machine package |
//! | --- | --- |
//! | `/qn`, `/quiet` | no UI exists, so the service cannot ask for consent — the install fails (1925 / 1603) |
//! | `/passive` | basic UI exists, so Windows Installer raises the UAC consent dialog and, once accepted, installs |
//!
//! So `/passive` it is. "Silent AND per-machine AND no prompt" is not a thing
//! Windows offers; one prompt a release is the smallest honest cost.
//!
//! **Why this module launches `msiexec` itself instead of calling
//! `update.install(bytes)`.** Two reasons, both load-bearing:
//!
//! 1. *A declined UAC must not kill the app.* `tauri-plugin-updater`'s
//!    `install_inner` calls `ShellExecuteW` and then `std::process::exit(0)`
//!    immediately — it does not wait. The UAC prompt is raised by the Windows
//!    Installer service AFTER msiexec starts, i.e. after we are already gone.
//!    Decline it and the machine is left with no Spaceadom running until the
//!    next logon. Owning the launch lets us WAIT on msiexec: a decline returns
//!    1602 to a process that is still alive, still hooked, still in the tray.
//! 2. *The install mode is not per-installer.* `plugins.updater.windows.installMode`
//!    is ONE value shared by both legs (verified in tauri-plugin-updater
//!    2.11.0 `src/config.rs`: `WindowsUpdateInstallMode::msiexec_args()` →
//!    `/quiet` for Quiet, `nsis_args()` → `/S`), and `UpdaterBuilder` exposes
//!    no runtime override — only `installer_args`, which are APPENDED. Turning
//!    the config to `passive` for the MSI's sake would swap the proven NSIS
//!    leg from `/S` to `/P` and put a progress window on every NSIS user's
//!    screen. Composing the MSI command ourselves leaves the NSIS leg exactly
//!    as it shipped and was proved in 1.0.99 → 1.0.100.
//!
//! The signature is still verified by the plugin: `Update::download` calls
//! `verify_signature` before it hands the bytes back (2.11.0
//! `src/updater.rs:740`), and the MSI updater artifact is a raw `.msi`, not a
//! zip (`scripts/write-updater-manifests.ps1`), so there is nothing to
//! unpack. We write those verified bytes to `%TEMP%` and run:
//!
//! ```text
//! msiexec.exe /i "<%TEMP%>\Spaceadom-1.0.X-installer.msi" /passive /norestart REBOOT=ReallySuppress AUTOLAUNCHAPP=True LAUNCHAPPARGS="--autostart"
//! ```
//!
//! * `/passive` — progress bar only, and the UI level that lets the consent
//!   dialog appear. `/norestart` plus the `REBOOT=ReallySuppress` property
//!   because a keyboard utility must never reboot anyone's machine; the
//!   property is the load-bearing half (it suppresses `ScheduleReboot` inside
//!   the package), the switch is the command-line echo of it.
//! * `AUTOLAUNCHAPP=True` and `LAUNCHAPPARGS="--autostart"` — **this is the
//!   relaunch.** `wix/main.wxs` carries the stock bundler pair
//!   `<Property Id="AUTOLAUNCHAPP" Secure="yes"/>` (Secure so it survives the
//!   client → service hop of a per-machine install) and
//!   `<Custom Action="LaunchApplication" After="InstallFinalize">AUTOLAUNCHAPP AND NOT Installed</Custom>`,
//!   whose action is `Impersonate="yes" FileKey="Path" ExeCommand="[LAUNCHAPPARGS]"
//!   Return="asyncNoWait"` — immediate, so it runs in the non-elevated client
//!   and the app comes back with the USER's token, never SYSTEM. `NOT Installed`
//!   is satisfied because `Product Id="*"` mints a new ProductCode every build,
//!   so a MajorUpgrade is a fresh install of a new product. `--autostart` gives
//!   the same logon-shaped relaunch the NSIS leg gets: no dashboard popping up
//!   at whatever hour the daily check fired.
//!
//! PROBLEM 127's `util:CloseApplication` terminates the running spaceadom.exe
//! early in the sequence, so on the success path `msiexec` never returns to us
//! — we are killed mid-wait, exactly as the NSIS leg's `taskkill` does it, and
//! the single-instance mutex is released by the kernel long before
//! `LaunchApplication` runs after `InstallFinalize`.
//!
//! **UAC declined** (1602) or refused (1925): one log line, the app carries on
//! untouched, and the next daily tick tries again. There is no dialog, no
//! toast and no badge — by construction, since this module owns no UI. The
//! full explanation is logged once per process; later declines get a short
//! line, so a user who says no every day does not fill the log with it.
//!
//! **Proving it without installing.** The `.msi` must NOT be installed on the
//! owner's machine: that machine's NSIS copy lives in `%LOCALAPPDATA%` and, on
//! any build predating the `wix/main.wxs` fix, the `.msi` adopts that folder
//! (PROBLEM 244). So the MSI leg's proof here is a unit test over the composed
//! command plus a dry run: with the dev-endpoint override file present, an MSI
//! copy logs `updater: MSI leg would run: …` and installs NOTHING. A real
//! end-to-end proof needs a second machine and no override file — the recipe
//! is in PROBLEM 246.
//!
//! **What actually happens on the NSIS leg.** `tauri-plugin-updater` fetches
//! the manifest, compares semver, downloads the `setup.exe` INTO MEMORY,
//! verifies its minisign signature against the public key baked into this
//! exe (`plugins.updater.pubkey`), writes it to `%TEMP%`, then runs
//!
//! ```text
//! Spaceadom-1.0.X-installer.exe /S /UPDATE /R /ARGS --autostart
//! ```
//!
//! and calls `std::process::exit(0)` — the plugin exits the process itself,
//! it does not ask. `/S` is silent (`installMode: "quiet"` in
//! tauri.conf.json), `/UPDATE` tells Tauri's NSIS script this is an upgrade
//! (no new shortcuts, no data removal), `/R` makes the installer relaunch the
//! app when it is done, and `/ARGS …` is what it relaunches with. The
//! `--autostart` is ours (`installer_args`, which the plugin appends after
//! `/ARGS`): the relaunched app then behaves like a logon launch — hook and
//! engine live at once, windows after the settle, and NO dashboard popping up
//! at whatever hour the daily check happened to fire.
//!
//! **Why the relaunch comes back at all — PROBLEM 233 / `restart_app`.**
//! `AppHandle::restart()` is banned here: it spawns the new process while the
//! old one still holds the single-instance mutex, the newcomer forwards its
//! arguments to the dying instance and quits, and the app never returns. The
//! updater's shape is the safe one by construction — the ORDER is exit first,
//! launch later: this process is gone within milliseconds of `ShellExecuteW`
//! returning; the installer then runs `installer-hooks.nsh`'s PREINSTALL
//! (`taskkill /F /T /IM spaceadom.exe` + 1.5 s, PROBLEM 127), copies the
//! files, and only THEN `/R` starts the new exe — seconds after the mutex was
//! released by the kernel. `on_before_exit` runs before the installer is even
//! started: it stops the keyboard hook (`hook::stop_hook`, the same call the
//! tray's Exit makes) so the incoming instance never shares the keyboard with
//! the outgoing one. Config needs no flush — every change is written to disk
//! the moment it is made (`config::save`), nothing is held for exit.
//!
//! **Failures are silent to the user and loud in the log.** Offline, a 404
//! (no release yet), a bad signature, a malformed manifest: all land in one
//! `updater: check failed` line and nothing else happens. The check runs on
//! its own thread (`st-updater`), never on the main thread, never before the
//! 10 s settle, and a failure blocks nothing.
//!
//! **The escape hatch** is one config field, `auto_update` (default true,
//! no Settings row by the owner's decision — "no switches"): set it to
//! `false` in `%APPDATA%\Spaceadom\config.json` and the check logs that it
//! was skipped. Its default is tested on both paths like `send_logs`.
//!
//! **The dev override** is a file named `st-updater-endpoint.txt` beside the
//! exe. Its first line replaces the manifest URL, and while it exists the
//! HTTP client accepts a self-signed certificate — that is how the 1.0.99 →
//! 1.0.100 proof was run against a localhost HTTPS server, with the shipped
//! config's `dangerousInsecureTransportProtocol` left OFF. Chosen over an
//! environment variable because a Run-key launch inherits no shell, and over
//! a config key because the file lives where only someone who can already
//! replace the exe can write. The kind gate still applies with the override
//! on: an Unknown or MSI copy skips regardless of the file.
//!
//! **"Updated to 1.0.X"** — `last-run-version.txt` in the data dir records
//! the version that last *showed* the dashboard. On a launch whose version
//! differs, the notice is held in memory until the dashboard asks
//! (`get_update_notice`), so a quiet `--autostart` relaunch does not lose it;
//! the file is only rewritten when the toast has actually been handed over.
//! It is deliberately NOT a config.json field: the proof compares the
//! config's SHA-256 across the update, and a version stamp written into it
//! at every first launch would make that check meaningless.

//! ---------------------------------------------------------------------------
//!
//! **PROBLEM 249 adds three things on top of all of the above, and none of them
//! changes a single decision the daily path already makes.**
//!
//! 1. **A MANUAL check** (`check_for_updates_now`, and the tray's "Check for
//!    updates"). It runs the SAME `plan()` → same manifest → same installer
//!    routing as the daily one. Two deliberate differences, both because a
//!    manual check is a user asking out loud:
//!    * it runs even when `auto_update` is `false`, **and it may install.**
//!      `auto_update: false` means "never do it behind my back"; a click is not
//!      behind anybody's back. The alternative — report-only — would leave a
//!      user who set the flag once with no way at all to take a fix, which is
//!      the "a control that does nothing is worse than a missing control" rule
//!      pointed at the updater.
//!    * it ignores `hold_updates_until` (below) and clears it on the way past.
//!    It is rate-limited to one run per 30 s and one at a time, so a
//!    double-click cannot start two downloads.
//!
//! 2. **ROLLBACK.** Every installer this app downloads is archived to
//!    `%APPDATA%\Spaceadom\rollback\Spaceadom-<ver>-installer.(exe|msi)` before
//!    it is run. At each launch the archive is pruned to at most TWO files: the
//!    installer for the version that is running (the seed — when we later leave
//!    this version, that file becomes the way back) and the newest installer
//!    OLDER than it (the rollback target). Nothing else in that directory is
//!    ever touched, and nothing outside it is ever touched.
//!
//!    *Why two and not one.* One file cannot do it. At the moment we install
//!    N we hold the bytes for N, never for the version we are leaving — so a
//!    single slot has to choose between being the seed and being the target,
//!    and it ends up alternating: rollback available after one update, gone
//!    after the next. Two slots make it available after every update from the
//!    second one onward. The cost is one extra installer on disk (~6 MB NSIS,
//!    ~10 MB MSI).
//!
//!    The FIRST auto-update therefore has no rollback and must not pretend to:
//!    the copy being left was installed by hand and its installer was never
//!    ours to keep. `rollback_available()` returns `None` and the UI shows
//!    nothing.
//!
//!    After a rollback the updater must not turn round and re-install the
//!    version just removed on the next daily tick. `rollback\hold.json` records
//!    `until_ms` (24 h out) and the version rolled back to; the DAILY check
//!    obeys it, a MANUAL check overrides and clears it.
//!
//! 3. **WHAT'S NEW** — `release_notes.rs`. Independent of everything here
//!    except two reads: `peek_update_notice()` (the version PROBLEM 245 already
//!    detected as "first launch after a version change") and
//!    `update_was_a_rollback()`.
//!
//! **The one thing outside this file that rollback needs**, and it is a single
//! JSON value: `bundle.windows.allowDowngrades` in `tauri.conf.json` is
//! **`false`** today. It is NOT under `nsis` — it is one key on
//! `bundle.windows` and it feeds BOTH legs (`tauri-utils` 2.9.3
//! `WindowsConfig::allow_downgrades`; `wix/main.wxs` already branches on it as
//! `{{#if allow_downgrades}}` → `<MajorUpgrade AllowDowngrades="yes"/>`, so the
//! WiX template needs no edit at all). With it `false` the generated
//! `installer.nsi` compiles a `Section EarlyChecks` that aborts a **silent**
//! downgrade, and the MSI gets `DowngradeErrorMessage` instead of
//! `AllowDowngrades="yes"`. See PROBLEM 249 for the measurement and for why
//! flipping it only helps installers built AFTER the flip.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

/// How THIS copy of Spaceadom was put on disk. See the module doc table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallKind {
    /// `setup.exe`, per-user, `%LOCALAPPDATA%\Spaceadom`. The recommended one.
    Nsis,
    /// `.msi`, per-machine, Program Files.
    Msi,
    /// Neither — a dev build or a hand-copied folder. Never updated.
    Unknown,
}

/// What `run_check` decided to do, and why. Pure, so it is testable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Plan {
    Check { url: String, dev_override: bool },
    Skip { reason: String },
}

/// The manifest for NSIS installs. `releases/latest/download/<asset>` is
/// GitHub's stable redirect to the newest non-prerelease release's asset, so
/// this URL never changes between versions. Written by release.yml.
pub const NSIS_MANIFEST: &str =
    "https://github.com/nur-arpon/Spaceadom/releases/latest/download/latest.json";
/// The manifest for MSI installs. Published by release.yml beside the other
/// one, consumed by nobody while `MSI_AUTO_UPDATE` is false.
pub const MSI_MANIFEST: &str =
    "https://github.com/nur-arpon/Spaceadom/releases/latest/download/latest-msi.json";
/// The dev override, beside the exe. First line = manifest URL.
///
/// **REVIEW FIXES 2026-09-05 (MEDIUM): read ONLY in a debug build** — see
/// [`read_override`] for why. **This changes a documented procedure**:
/// CLAUDE.md's "To test an update locally" recipe drops this file beside an
/// INSTALLED (release) exe, and a release build now ignores it entirely. Use a
/// debug build for that test, or delete the `cfg!(debug_assertions)` gate for
/// the duration of the experiment and put it back.
pub const OVERRIDE_FILE: &str = "st-updater-endpoint.txt";
/// Where the "last version that showed the dashboard" lives. Data dir, not
/// config.json — see the module doc.
const LAST_RUN_FILE: &str = "last-run-version.txt";

/// The MSI leg is ON (owner's decision, PROBLEM 246). Setting this to `false`
/// puts an MSI copy back to "detected, never driven" without touching anything
/// else — the `Plan::Skip` arm below still carries the explanation.
const MSI_AUTO_UPDATE: bool = true;

/// The msiexec switches for an MSI self-update, in order.
///
/// `/passive` is not a style choice. Measured on this machine against msiexec
/// 5.00.10011.00 by reading `/L*v` logs: a `/qn` run creates no UI objects at
/// all, a `/passive` run creates them ("Font created … MS Shell Dlg"). No UI
/// means no way for the Windows Installer service to raise the UAC consent
/// dialog, and a per-machine package launched from a non-elevated app then
/// simply fails. `/passive` is the *smallest* UI level that can still ask.
///
/// `REBOOT=ReallySuppress` is the half that actually binds — it suppresses the
/// package's own `ScheduleReboot`/`ForceReboot`. `/norestart` is the
/// command-line echo of the same intent. A keyboard utility does not get to
/// reboot somebody's machine to finish a background update.
const MSI_SWITCHES: [&str; 3] = ["/passive", "/norestart", "REBOOT=ReallySuppress"];

/// The relaunch pair, consumed by `wix/main.wxs`'s stock `LaunchApplication`
/// custom action (`AUTOLAUNCHAPP AND NOT Installed`, immediate + impersonated,
/// so the app comes back as the USER and not as SYSTEM). `--autostart` makes
/// the returning instance logon-shaped: hook and engine live, no dashboard.
const MSI_RELAUNCH: [&str; 2] = ["AUTOLAUNCHAPP=True", "LAUNCHAPPARGS=\"--autostart\""];

/// First check: the autostart settle is 10 s (`lib.rs::AUTOSTART_SETTLE`);
/// this waits past it so the check never competes with window creation.
const FIRST_CHECK_DELAY: Duration = Duration::from_secs(15);
/// Then daily. A process that lives for weeks (this one does — it autostarts)
/// keeps checking every 24 h from the same thread.
const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
/// Per-request timeout. The manifest is ~1 KB and the installer ~6 MB; a
/// minute is generous and still short enough that a stalled connection does
/// not pin the thread past the next interval.
const HTTP_TIMEOUT: Duration = Duration::from_secs(60);

// --- PROBLEM 249 ------------------------------------------------------------

/// The ONE event every stage of every check publishes. Payload: `UpdateStatus`.
pub const EVENT_UPDATE_STATUS: &str = "update-status";

/// The states an `UpdateStatus` can carry. **The UI contract is: switch on the
/// ones you handle and fall back to showing `message` verbatim for anything
/// else** — `message` is always a finished, user-facing sentence, so an
/// unrecognised state can never leave a blank panel.
pub const STATE_CHECKING: &str = "checking";
pub const STATE_UP_TO_DATE: &str = "up_to_date";
pub const STATE_DOWNLOADING: &str = "downloading";
pub const STATE_INSTALLING: &str = "installing";
pub const STATE_ERROR: &str = "error";
/// A manual check ran less than `MANUAL_MIN_GAP` ago, or one is still running.
/// Deliberately NOT folded into `error`: nothing failed, and painting a red
/// failure for an impatient second click would be a lie about the app's health.
pub const STATE_BUSY: &str = "busy";
/// This copy is a dev build or a hand-copied folder, or it is an `.msi` while
/// `MSI_AUTO_UPDATE` is off. Also not an error: the app is fine, it simply is
/// not a copy this updater is allowed to replace.
pub const STATE_NOT_ELIGIBLE: &str = "not_eligible";

/// The smallest gap between two MANUAL checks. A click is cheap; a GitHub
/// round trip plus a 6 MB download is not, and a user who taps the tray item
/// four times must not start four of them.
const MANUAL_MIN_GAP: Duration = Duration::from_secs(30);

/// How long a rollback holds the DAILY check off. 24 h — long enough that the
/// machine gets a full day on the older build, short enough that a user who
/// rolled back once and then forgot about it is not stranded on it forever.
/// A manual check overrides it at any time.
const ROLLBACK_HOLD: Duration = Duration::from_secs(24 * 60 * 60);

/// Where archived installers live: `%APPDATA%\Spaceadom\rollback\`. Created by
/// this feature, pruned by this feature, and **the only directory any of this
/// code ever deletes from** — and then only files whose names it wrote itself
/// (`Spaceadom-<version>-installer.exe|.msi`).
const ROLLBACK_DIR: &str = "rollback";
/// The 24 h hold + pinned version, inside `ROLLBACK_DIR`.
const HOLD_FILE: &str = "hold.json";

/// The archive filename prefix and suffix. Split out so `parse_archive_name`
/// and `archive_name` cannot drift apart.
const ARCHIVE_PREFIX: &str = "Spaceadom-";
const ARCHIVE_SUFFIX: &str = "-installer";

/// **REVIEW FIXES 2026-09-05 (H1).** The detached minisign signature that now
/// sits beside every archived installer: `Spaceadom-1.0.99-installer.exe.sig`.
///
/// Deliberately an EXTRA extension rather than a replacement, so
/// `parse_archive_name` — which is the predicate deciding what this feature is
/// allowed to delete — still refuses to recognise it as an installer. The
/// prune removes a signature only as the companion of an installer it is
/// already removing.
const ARCHIVE_SIG_EXT: &str = ".sig";

static LAST_MANUAL_CHECK: Mutex<Option<std::time::Instant>> = Mutex::new(None);
static MANUAL_IN_FLIGHT: AtomicBool = AtomicBool::new(false);

/// THE decision, pure. `msi_registration_dir` is the normalised
/// `InstallLocation` of an HKLM "Spaceadom" entry with an `MsiExec /X`
/// uninstall string, if one exists; `exe_dir` is this exe's folder.
pub(crate) fn classify_install(
    has_uninstall_exe: bool,
    msi_registration_dir: Option<&str>,
    exe_dir: &str,
) -> InstallKind {
    // NSIS's own artefact. WiX never writes one, and PROBLEM 244's "MSI
    // registered for the NSIS folder" state must still read as NSIS because
    // NSIS is what put the files there.
    if has_uninstall_exe {
        return InstallKind::Nsis;
    }
    if let Some(dir) = msi_registration_dir {
        if same_dir(dir, exe_dir) {
            // REVIEW FIXES 2026-09-05 (MEDIUM) — A PER-MACHINE .MSI DOES NOT
            // LIVE IN THE USER PROFILE, EVER.
            //
            // `uninstall.exe` is the only NSIS artefact this function looks
            // for, and it is a file — deletable, and missing in every
            // half-uninstalled or hand-copied NSIS folder. When it is gone,
            // an MSI registration whose InstallLocation happens to BE the
            // per-user folder made this function answer `Msi`, and that answer
            // is what points the updater at `latest-msi.json` and, eventually,
            // at `msiexec` aimed straight at `%LOCALAPPDATA%\Spaceadom`. That
            // is PROBLEM 244's shape exactly: an MSI product registered for
            // the NSIS folder, and a removal aimed by the thing being removed.
            //
            // A genuine per-machine install is in Program Files. A path under
            // `\Users\` or `\AppData\` is a per-user location by definition, so
            // whatever registered it, this copy is not one an `.msi` may
            // replace. The answer is `Unknown` — never updated — which is the
            // safe end of the three.
            if is_per_user_location(exe_dir) {
                return InstallKind::Unknown;
            }
            return InstallKind::Msi;
        }
    }
    InstallKind::Unknown
}

/// Pure. Is `dir` inside a user profile rather than a per-machine location?
///
/// Shape-based, so the decision stays testable without a machine: a
/// per-machine Windows Installer package installs under Program Files, and
/// `\Users\…` / `\AppData\…` is where per-user software goes.
/// `detect_install_kind` adds a second, environment-based check on top of this
/// for the unusual case of a profile root that is not under `\Users\`.
pub(crate) fn is_per_user_location(dir: &str) -> bool {
    let d = dir.trim().trim_matches('"').replace('/', "\\").to_ascii_lowercase();
    d.contains("\\users\\") || d.contains("\\appdata\\")
}

/// The EXACT msiexec parameter string for an MSI self-update. Pure, so the
/// command that would run can be asserted in a unit test and printed in a dry
/// run without a machine, an installer or an elevation prompt anywhere near it.
///
/// Returned as one verbatim string rather than a `Vec<String>` on purpose: it
/// is handed to `Command::raw_arg`, because msiexec does NOT parse its command
/// line with `CommandLineToArgvW` and Rust's normal argument quoting would
/// mangle `LAUNCHAPPARGS="--autostart"` into `LAUNCHAPPARGS=\"--autostart\"`.
pub(crate) fn msi_parameters(msi_path: &str) -> String {
    let mut parts = vec![format!("/i \"{msi_path}\"")];
    parts.extend(MSI_SWITCHES.iter().map(|s| (*s).to_string()));
    parts.extend(MSI_RELAUNCH.iter().map(|s| (*s).to_string()));
    parts.join(" ")
}

fn same_dir(a: &str, b: &str) -> bool {
    let norm = |s: &str| {
        s.trim()
            .trim_matches('"')
            .trim_end_matches(['\\', '/'])
            .to_ascii_lowercase()
    };
    !a.trim().is_empty() && norm(a) == norm(b)
}

/// Which URL to check, or why not. Pure.
pub(crate) fn plan(kind: InstallKind, override_url: Option<&str>) -> Plan {
    let override_url = override_url.map(str::trim).filter(|s| !s.is_empty());
    match kind {
        InstallKind::Unknown => Plan::Skip {
            reason: "this copy was not installed by setup.exe or the .msi (no uninstall.exe \
                     beside the exe, no MSI product registered for its folder) — a dev build \
                     or a copied folder is never updated"
                .into(),
        },
        // Only reachable if MSI_AUTO_UPDATE is flipped back to false. Kept so
        // that switch stays a one-word change with its reason attached.
        InstallKind::Msi if !MSI_AUTO_UPDATE => Plan::Skip {
            reason: "this copy is the per-machine .msi and MSI_AUTO_UPDATE is off — a \
                     per-machine upgrade needs elevation, and Windows Installer can only \
                     raise the UAC prompt at a UI level of /passive or higher (PROBLEM 246). \
                     Update by running the newer .msi yourself"
                .into(),
        },
        InstallKind::Msi => Plan::Check {
            url: override_url.unwrap_or(MSI_MANIFEST).to_string(),
            dev_override: override_url.is_some(),
        },
        InstallKind::Nsis => Plan::Check {
            url: override_url.unwrap_or(NSIS_MANIFEST).to_string(),
            dev_override: override_url.is_some(),
        },
    }
}

// ---------------------------------------------------------------------------
// Evidence gathering (Windows only; the rest of the file is pure).
// ---------------------------------------------------------------------------

/// The registered `InstallLocation` of an HKLM Uninstall entry named
/// "Spaceadom" whose uninstall string is `MsiExec /X…` — i.e. a real Windows
/// Installer product, not our NSIS entry (that one lives in HKCU and points
/// at `uninstall.exe`). Both native and WOW6432Node roots, read-only, never
/// HKCU (virtualised for the agent shell — PROBLEM 143).
#[cfg(windows)]
fn msi_registration_dir() -> Option<String> {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    for root in [
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
        r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
    ] {
        let Ok(key) = hklm.open_subkey(root) else { continue };
        for name in key.enum_keys().flatten() {
            let Ok(entry) = key.open_subkey(&name) else { continue };
            let display: String = entry.get_value("DisplayName").unwrap_or_default();
            if display != "Spaceadom" {
                continue;
            }
            let uninstall: String = entry.get_value("UninstallString").unwrap_or_default();
            if !crate::rival_install::is_msiexec_uninstall(&uninstall) {
                continue;
            }
            let location: String = entry.get_value("InstallLocation").unwrap_or_default();
            return Some(crate::rival_install::normalize_dir(&location));
        }
    }
    None
}

/// Look at the disk and the registry and say how this copy was installed.
/// Logs every input to the decision, so a wrong verdict can be argued from
/// the log instead of re-measured.
#[cfg(windows)]
pub fn detect_install_kind() -> InstallKind {
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => {
            log::warn!("updater: current_exe() failed ({e}) — install kind Unknown, no check");
            return InstallKind::Unknown;
        }
    };
    let dir = exe.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let has_uninstall_exe = dir.join("uninstall.exe").is_file();
    let msi_dir = msi_registration_dir();
    let exe_dir = dir.to_string_lossy().to_string();
    let mut kind = classify_install(has_uninstall_exe, msi_dir.as_deref(), &exe_dir);

    // REVIEW FIXES 2026-09-05 (MEDIUM) — the second, environment-based half of
    // the per-user check. `classify_install`'s test is on the path's SHAPE
    // (`\Users\`, `\AppData\`) and is pure; this catches the machine whose
    // profile root is somewhere else entirely (`D:\Profiles\bob`), which the
    // shape test cannot see. Both are logged, and either one is enough.
    if kind == InstallKind::Msi {
        let lower = exe_dir.replace('/', "\\").to_ascii_lowercase();
        for var in ["LOCALAPPDATA", "APPDATA", "USERPROFILE"] {
            let Ok(root) = std::env::var(var) else { continue };
            let root = root.replace('/', "\\").trim_end_matches('\\').to_ascii_lowercase();
            if root.len() >= 4 && lower.starts_with(&format!("{root}\\")) {
                log::warn!(
                    "updater: an MSI product is registered for this exe's folder, but the \
                     folder is inside %{var}% ({root}) — a per-machine .msi does not install \
                     into a user profile, so this is PROBLEM 244's shape (an MSI registration \
                     adopting the per-user NSIS folder) rather than a real MSI install. \
                     Downgrading the verdict to Unknown: this copy will NOT be auto-updated by \
                     anything, which is the safe answer."
                );
                kind = InstallKind::Unknown;
                break;
            }
        }
    }

    log::info!(
        "updater: install kind decided — {kind:?} (uninstall.exe beside the exe: \
         {has_uninstall_exe}; MSI product registered for this folder: {msi_dir:?}; exe dir: \
         {exe_dir}; tauri bundle_type: {:?})",
        tauri::utils::platform::bundle_type()
    );
    kind
}

#[cfg(not(windows))]
pub fn detect_install_kind() -> InstallKind {
    InstallKind::Unknown
}

/// The dev override file's first line, if the file exists beside the exe.
///
/// **REVIEW FIXES 2026-09-05 (MEDIUM) — DEBUG BUILDS ONLY.**
///
/// This file does two things at once: it replaces the manifest URL, and it
/// turns on `danger_accept_invalid_certs`. In a RELEASE build that combination
/// is a remote-code-execution primitive with a text file as its key —
/// anything that can drop `st-updater-endpoint.txt` beside the exe (the exe
/// sits in `%LOCALAPPDATA%` on every NSIS install, writable by this user)
/// points the updater at a server of its choosing over a connection whose
/// certificate is not checked. The minisign signature is still verified, so an
/// attacker cannot get a payload INSTALLED — but "one more control has to
/// fail" is not the same as "this cannot happen", and a debug-build gate costs
/// the feature nothing: PROBLEM 245's worked example and PROBLEM 246's dry run
/// are both developer procedures.
///
/// The gate is `cfg!(...)` rather than `#[cfg(...)]` so the whole function
/// still type-checks and is still read by the compiler in release, and the
/// release build simply returns `None`.
fn read_override() -> Option<String> {
    if !cfg!(debug_assertions) {
        return None;
    }
    let exe = std::env::current_exe().ok()?;
    let path = exe.parent()?.join(OVERRIDE_FILE);
    let text = std::fs::read_to_string(&path).ok()?;
    let line = text.lines().next().unwrap_or("").trim().to_string();
    log::warn!(
        "updater: DEV OVERRIDE in effect — {} exists beside the exe; manifest URL taken \
         from it ({line}) and a self-signed certificate will be accepted. Delete the file \
         to return to GitHub releases.",
        path.display()
    );
    Some(line)
}

// ---------------------------------------------------------------------------
// The check itself.
// ---------------------------------------------------------------------------

/// Spawn the `st-updater` thread: one check after the settle, then daily.
/// Never blocks the caller; a thread that cannot be spawned is logged and
/// the session simply has no update checks.
pub fn schedule(app: tauri::AppHandle) {
    let spawned = std::thread::Builder::new()
        .name("st-updater".into())
        .spawn(move || {
            log::info!(
                "updater: scheduled — first check in {}s (after the {}s autostart settle), \
                 then every {}h, on this thread and never on the main thread",
                FIRST_CHECK_DELAY.as_secs(),
                crate::AUTOSTART_SETTLE.as_secs(),
                CHECK_INTERVAL.as_secs() / 3600
            );
            std::thread::sleep(FIRST_CHECK_DELAY);
            // PROBLEM 249 — prune the rollback archive first, on this thread,
            // past the settle. It is the only moment at which "which installer
            // is the seed and which is the way back" has a single answer: the
            // running version is decided and no download is in flight.
            prune_rollback_archive(&app.package_info().version.to_string());
            loop {
                run_check(&app);
                std::thread::sleep(CHECK_INTERVAL);
            }
        });
    if spawned.is_err() {
        log::error!("updater: could not spawn the st-updater thread — no update checks this session");
    }
}

/// One check: config gate → install kind → plan → plugin. Everything that can
/// fail is logged here; nothing propagates.
fn run_check(app: &tauri::AppHandle) {
    // PROBLEM 250 — the Microsoft Store build. FIRST gate, before the config
    // read, because in a package there is nothing here worth doing: the Store
    // owns updates, and installing a downloaded setup.exe from inside a package
    // would leave a SECOND, unpackaged Spaceadom beside this one, both hooking
    // the spacebar (PROBLEM 129). See src/packaged.rs.
    if crate::packaged::is_packaged() {
        log::info!(
            "updater: PACKAGED (MSIX) — no update check. The Microsoft Store owns updates \
             for this copy; running a downloaded setup.exe from inside a package would \
             install a SECOND, unpackaged Spaceadom beside it and put two keyboard hooks \
             on the machine (PROBLEM 129/250)."
        );
        return;
    }
    // PROBLEM 254 — the portable build. SECOND gate, immediately after the
    // packaged one and for the same class of reason: a portable copy was never
    // installed, so there is no `setup.exe` and no MSI product to hand an
    // update to. `detect_install_kind()` would already return `Unknown` and
    // `plan()` would already skip — but only by accident of what happens to be
    // beside the exe, and the log line it produces says nothing about portable
    // mode. An explicit gate is the difference between "no update, and here is
    // why" and "no update, work it out yourself". See src/portable.rs.
    if crate::portable::is_portable() {
        log::info!(
            "updater: PORTABLE — no update check. This copy was unzipped, not installed \
             (a {} marker sits beside the exe), so there is no installer of ours to re-run \
             and nothing that could replace this folder safely. Download a new zip when \
             you want a newer version (PROBLEM 254).",
            crate::portable::MARKER_FILE
        );
        return;
    }
    use tauri::Manager;
    let auto = app
        .state::<crate::commands::ConfigState>()
        .0
        .read()
        .map(|c| c.auto_update)
        .unwrap_or(true);
    if !auto {
        log::info!(
            "updater: auto_update is false in config.json — check skipped (the config-file \
             escape hatch, PROBLEM 245). A MANUAL check still runs and still installs: a \
             click is not something happening behind the user's back (PROBLEM 249)."
        );
        return;
    }
    // PROBLEM 249 — a rollback holds the DAILY check off for 24 h, so the
    // machine does not spend the evening being put straight back onto the
    // version the user just walked away from. A manual check overrides it.
    if let Some(hold) = read_hold() {
        if hold_blocks_daily_check(now_ms(), Some(&hold)) {
            log::info!(
                "updater: daily check skipped — a rollback to {} is holding updates until \
                 epoch-ms {} ({}). Use \"Check for updates\" to override it now.",
                hold.pinned_version,
                hold.until_ms,
                hold.reason
            );
            return;
        }
        log::info!(
            "updater: the rollback hold on {} has expired — checking normally again",
            hold.pinned_version
        );
        clear_hold();
    }
    let kind = detect_install_kind();
    match plan(kind, read_override().as_deref()) {
        Plan::Skip { reason } => log::info!("updater: no check — {reason}"),
        Plan::Check { url, dev_override } => {
            if let Err(e) = tauri::async_runtime::block_on(check_and_install(
                app,
                &url,
                dev_override,
                kind,
                false,
            )) {
                // One line, every failure class: offline, 404 before the first
                // signed release, a manifest that does not parse, a signature
                // that does not verify. Silent to the user by design.
                log::warn!(
                    "updater: check failed, silent to the user (offline / 404 / bad manifest / \
                     bad signature all land here): {e}"
                );
                // PROBLEM 249 — silent to the user still means silent: this
                // status is a TERMINAL one for a UI that may already be showing
                // a progress bar from the "downloading" event above. A daily
                // check that dies mid-download must not leave the dashboard
                // stuck at 60% forever.
                emit_always(
                    app,
                    UpdateStatus::err(
                        &app.package_info().version.to_string(),
                        "The update couldn't be completed. Nothing changed, and Spaceadom \
                         will try again tomorrow.",
                    ),
                );
            }
        }
    }
}

/// One check-and-maybe-install. `manual` is true only for
/// `check_for_updates_now` / the tray item, and changes exactly two things:
/// the "you're on the latest" status is emitted and returned (the daily check
/// stays silent, so the tray label never changes on its own at 3 a.m.), and a
/// rollback hold has already been cleared by the caller.
async fn check_and_install(
    app: &tauri::AppHandle,
    url: &str,
    dev_override: bool,
    kind: InstallKind,
    manual: bool,
) -> Result<UpdateStatus, tauri_plugin_updater::Error> {
    use tauri_plugin_updater::UpdaterExt;

    let current = app.package_info().version.to_string();
    let parsed = match tauri::Url::parse(url) {
        Ok(u) => u,
        Err(e) => {
            log::warn!("updater: manifest URL {url:?} does not parse ({e}) — no check");
            return Ok(emit_status(
                app,
                UpdateStatus::err(&current, format!("The update address is not usable ({e}).")),
                manual,
            ));
        }
    };
    log::info!("updater: checking {url} for a release newer than {current}");

    let exit_handle = app.clone();
    let mut builder = app
        .updater_builder()
        .endpoints(vec![parsed])?
        .timeout(HTTP_TIMEOUT)
        // NSIS leg only — appended after `/ARGS`, so it becomes the relaunched
        // app's argv: a quiet, logon-shaped relaunch (see the module doc). The
        // MSI leg never reaches `update.install()`, so neither this nor
        // `on_before_exit` below has any effect there; the MSI equivalent is
        // `LAUNCHAPPARGS="--autostart"` in `MSI_RELAUNCH`.
        .installer_args(["--autostart"])
        .on_before_exit(move || {
            log::info!(
                "updater: on_before_exit — stopping the keyboard hook and running Tauri's \
                 exit cleanup; the installer takes over from here and relaunches with \
                 --autostart. Config needs no flush: every change is already on disk."
            );
            // REVIEW FIXES 2026-09-05 (H5) — A SUCCESSFUL UPDATE IS NOT A
            // STARTUP CRASH.
            //
            // `tauri-plugin-updater` calls this and then `std::process::exit(0)`
            // itself: no `RunEvent::Exit`, so `lib.rs`'s RunEvent closure never
            // runs and `session_end.rs` never sees a `WM_ENDSESSION`. Neither of
            // safe mode's two clean-exit paths fires. An update that arrives
            // inside the first 30 seconds of a launch — which is exactly when
            // it does arrive, `FIRST_CHECK_DELAY` is 15 s — therefore left this
            // launch's `+1` standing on disk. Three updates in three days and
            // the fourth launch comes up with no keyboard hook, blaming a crash
            // that never happened.
            //
            // Here rather than beside `update.install(bytes)`: this callback
            // runs only when the plugin is actually about to exit, so a failed
            // install that RETURNS leaves the counter honest.
            crate::safe_mode::note_clean_exit();
            exit_handle.cleanup_before_exit();
        });
    if dev_override {
        builder = builder.configure_client(|c| c.danger_accept_invalid_certs(true));
    }
    let updater = builder.build()?;

    let Some(update) = updater.check().await? else {
        log::info!("updater: no update — {current} is the newest release on the manifest");
        return Ok(emit_status(
            app,
            UpdateStatus {
                state: STATE_UP_TO_DATE,
                current: current.clone(),
                latest: Some(current.clone()),
                message: format!("You're on the latest version ({current})."),
                progress: None,
            },
            manual,
        ));
    };
    log::info!(
        "updater: UPDATE AVAILABLE — {} is newer than the running {} — downloading {}",
        update.version,
        update.current_version,
        update.download_url
    );

    let latest = update.version.clone();
    // Always emitted, manual or daily: a download that is going to end in the
    // process exiting is something a visible dashboard should be allowed to say
    // out loud, whoever started it.
    emit_always(
        app,
        UpdateStatus {
            state: STATE_DOWNLOADING,
            current: current.clone(),
            latest: Some(latest.clone()),
            message: format!("Downloading version {latest}…"),
            progress: Some(0),
        },
    );

    let mut got: usize = 0;
    let mut next_mark: u64 = 25;
    // The UI wants smoother progress than the log does; the log keeps its
    // original 25% marks so PROBLEM 245's worked example still reads the same.
    let mut next_emit: u64 = 5;
    let bytes = update
        .download(
            |chunk, total| {
                got += chunk;
                if let Some(total) = total.filter(|t| *t > 0) {
                    let pct = (got as u64 * 100) / total;
                    while pct >= next_mark && next_mark <= 100 {
                        log::info!("updater: download {next_mark}% ({got} of {total} bytes)");
                        next_mark += 25;
                    }
                    if pct >= next_emit {
                        while next_emit <= pct && next_emit <= 100 {
                            next_emit += 5;
                        }
                        emit_always(
                            app,
                            UpdateStatus {
                                state: STATE_DOWNLOADING,
                                current: current.clone(),
                                latest: Some(latest.clone()),
                                message: format!("Downloading version {latest}…"),
                                progress: Some(pct.min(100) as u8),
                            },
                        );
                    }
                }
            },
            || log::info!("updater: download finished"),
        )
        .await?;

    // PROBLEM 249 — archive the installer we are about to run, BEFORE running
    // it: on the NSIS leg the next statement never returns. This file is not
    // the way back from the version we are installing; it is the way back from
    // the version AFTER it, which is why the prune at launch keeps two.
    //
    // REVIEW FIXES 2026-09-05 (H1) — `update.signature` goes with it. It is the
    // base64 minisign signature out of the manifest that `Update::download`
    // has just checked THESE bytes against; keeping it is what lets the same
    // check run again months later, before the file is executed elevated.
    archive_installer(&bytes, &latest, kind, &update.signature);

    // Emitted ONCE, here, before either leg is entered — because on the NSIS
    // leg the next call never returns and on the MSI leg the process is
    // normally killed mid-wait by util:CloseApplication. This is the last
    // moment at which anything can be said. `installing` is then also what the
    // function RETURNS on the paths that survive, so the return value and the
    // event carry the same sentence rather than two drafts of it.
    let installing = emit_always(
        app,
        UpdateStatus {
            state: STATE_INSTALLING,
            current: current.clone(),
            latest: Some(latest.clone()),
            message: format!("Installing version {latest}. Spaceadom will restart itself."),
            progress: Some(100),
        },
    );

    if kind == InstallKind::Msi {
        log::info!(
            "updater: {} bytes downloaded and the minisign signature verified against the \
             built-in public key — handing off to the MSI leg (PROBLEM 246)",
            bytes.len()
        );
        // Reached only when msiexec came back to us at all — a declined UAC, a
        // failure, the dry run, or the rare "installed but did not close us".
        // On the ordinary success path util:CloseApplication kills this process
        // mid-wait and nothing below runs.
        return Ok(match install_msi(&bytes, &update.version, dev_override) {
            Ok(()) => installing,
            // `emit_always`, not `emit_status`: the UI has already been told
            // "downloading" and "installing", so it is OWED a terminal status
            // whether or not a human started this. Leaving a daily check's
            // declined UAC showing an eternal "Installing…" would be the
            // window-hidden bug of PROBLEM 135 in a progress bar.
            Err(why) => emit_always(app, UpdateStatus::err(&current, why)),
        });
    }

    log::info!(
        "updater: {} bytes downloaded and the minisign signature verified against the \
         built-in public key — installing SILENTLY now (setup.exe /S /UPDATE /R /ARGS \
         --autostart) and exiting; the installer relaunches Spaceadom when it is done",
        bytes.len()
    );
    // Exits the process on success (Windows). Returns only on failure — and a
    // failure is `?`-propagated, so this line is only ever reached on a
    // platform where `install` is a no-op.
    //
    // REVIEW FIXES 2026-09-05 (H5): the `safe_mode::note_clean_exit()` that
    // stops this exit being counted as a startup crash lives in the
    // `on_before_exit` callback above, which the plugin invokes immediately
    // before its own `process::exit(0)`. Deliberately not here — this call can
    // also return an error, and marking a clean exit for an install that then
    // failed would erase a real launch's record.
    update.install(bytes)?;
    Ok(installing)
}

// ---------------------------------------------------------------------------
// The MSI leg (PROBLEM 246).
// ---------------------------------------------------------------------------

/// True once a declined UAC has been explained in full. Later declines get one
/// short line instead: "log once, retry next day, no nag" (owner's words).
static MSI_DECLINE_EXPLAINED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// A user-facing sentence for each msiexec exit code we know by name. Pure, so
/// the wording is testable without an installer anywhere near it. `None` means
/// "the package installed" — that path has no message because the process is
/// normally already dead by then.
pub(crate) fn msiexec_reason(code: i32) -> String {
    match code {
        1602 => "The Windows permission prompt was declined, so nothing was installed. \
                 Spaceadom is untouched."
            .into(),
        1603 => "Windows Installer could not finish (error 1603). Nothing was changed."
            .into(),
        1618 => "Another installation is already running on this PC. Nothing was changed — \
                 try again when it has finished."
            .into(),
        1925 => "This copy was installed for everyone on the PC, so updating it needs \
                 administrator permission. Nothing was changed."
            .into(),
        other => format!("Windows Installer stopped with code {other}. Nothing was changed."),
    }
}

/// Run the downloaded, signature-verified `.msi`.
///
/// `bytes` have already been checked against the built-in public key by
/// `Update::download`. Everything here is deliberately OURS rather than
/// `update.install()` — see the module doc: the plugin exits the process the
/// instant it launches msiexec, which would mean a declined UAC leaves the
/// machine with no Spaceadom running at all.
///
/// `Ok(())` means the package really did install (and only failed to close us);
/// `Err(sentence)` means nothing was installed and says why in plain words.
/// **REVIEW FIXES 2026-09-05 (H1).** The staging directory and the
/// write-then-recompute guard.
///
/// What was wrong: the verified bytes were written to a **fixed, predictable**
/// path — `%TEMP%\Spaceadom-1.0.X-installer.msi` — and then handed to msiexec,
/// which runs it elevated. Between `fs::write` and `Command::status` there is a
/// window in which any process running as this user can replace that file, and
/// the name it has to guess is printed in the log and derivable from the public
/// release feed. The signature check the plugin did protects the BYTES IN
/// MEMORY; it says nothing about what is on disk a moment later.
///
/// Three changes, each closing a different half of it:
///
/// 1. A **fresh, randomly-named subdirectory** of `%TEMP%`, created by us. The
///    path cannot be pre-created, pre-opened, or replaced by a symlink planted
///    in advance, because nothing else knows it.
/// 2. The **SHA-256 is taken from the in-memory bytes at write time** — the
///    same bytes minisign has already vouched for.
/// 3. The hash is **recomputed from the file on disk immediately before
///    `raw_arg`**, inside `run_msiexec`, and a mismatch refuses the launch
///    loudly instead of elevating whatever is there.
#[cfg(windows)]
fn install_msi(bytes: &[u8], version: &str, dev_override: bool) -> Result<(), String> {
    let stage = std::env::temp_dir().join(format!("spaceadom-update-{}", temp_dir_suffix()));
    if let Err(e) = std::fs::create_dir_all(&stage) {
        log::warn!(
            "updater: could not create the staging directory {} ({e}) — no update this time, \
             the next daily check retries",
            stage.display()
        );
        return Err("The update could not be saved to this PC's temporary folder.".into());
    }
    let msi_path = stage.join(format!("Spaceadom-{version}-installer.msi"));
    if let Err(e) = std::fs::write(&msi_path, bytes) {
        log::warn!(
            "updater: could not write the verified .msi to {} ({e}) — no update this time, \
             the next daily check retries",
            msi_path.display()
        );
        let _ = std::fs::remove_dir_all(&stage);
        return Err("The update could not be saved to this PC's temporary folder.".into());
    }
    // Taken from the bytes the plugin's minisign check passed, NOT from the
    // file — hashing the file would only prove the file equals itself.
    let expected = sha256_hex(bytes);
    log::info!(
        "updater: staged the verified .msi in a fresh private directory — {} ({} bytes, \
         SHA-256 {expected}). The hash is recomputed from disk immediately before msiexec is \
         launched, so a file swapped in behind us is refused instead of elevated (H1).",
        msi_path.display(),
        bytes.len()
    );

    let params = msi_parameters(&msi_path.to_string_lossy());

    // The dry run. With the dev-endpoint override file beside the exe, an MSI
    // copy prints the exact command and installs NOTHING — that is how this
    // leg is provable on a machine where installing the .msi is forbidden
    // (PROBLEM 244: it would adopt the live per-user NSIS folder).
    if dev_override {
        log::warn!("updater: MSI leg would run: msiexec.exe {params}");
        log::warn!(
            "updater: DRY RUN — the dev-endpoint override is present, so nothing was \
             installed. Delete {OVERRIDE_FILE} beside the exe for a real MSI update."
        );
        let _ = std::fs::remove_dir_all(&stage);
        return Err("Dry run: the developer endpoint override is in place, so nothing was \
                    installed."
            .into());
    }

    let guard_path = msi_path.clone();
    let outcome = run_msiexec(
        &params,
        version,
        Some(&msi_path),
        &move || sha256_guard(&guard_path, &expected),
    );
    // Reached only when nothing installed (`run_msiexec` cleans the file
    // itself); take the directory we created with it.
    if outcome.is_err() {
        let _ = std::fs::remove_dir_all(&stage);
    }
    outcome
}

/// The check `install_msi` hands to `run_msiexec`: re-read the staged file and
/// compare. Split out so the refusal wording lives beside the reason for it.
#[cfg(windows)]
fn sha256_guard(path: &std::path::Path, expected: &str) -> Result<(), String> {
    let on_disk = std::fs::read(path)
        .map_err(|e| format!("the staged installer could not be re-read from disk ({e})"))?;
    let actual = sha256_hex(&on_disk);
    if actual == expected {
        return Ok(());
    }
    Err(format!(
        "the staged installer changed between being written and being launched — expected \
         SHA-256 {expected}, found {actual}"
    ))
}

/// Launch msiexec with `params`, WAIT for it, and turn the exit code into a
/// verdict. Shared by the update leg and by `rollback_to_previous`, so the two
/// can never disagree about switches, quoting or what a decline means.
///
/// `cleanup` is deleted when nothing was installed; it is `None` for a rollback,
/// whose package is the ARCHIVE and must survive a declined prompt.
///
/// **REVIEW FIXES 2026-09-05 (H1)** — `guard` is the last integrity check
/// before the package is handed to an elevated installer, and it runs as the
/// statement immediately before `raw_arg`. The update leg passes a SHA-256
/// recompute of its staged file; the rollback leg passes a re-verification of
/// the archive's minisign signature. Both legs go through the same door on
/// purpose: a future third caller cannot forget to check, because the
/// parameter has no default.
#[cfg(windows)]
fn run_msiexec(
    params: &str,
    version: &str,
    cleanup: Option<&std::path::Path>,
    guard: &dyn Fn() -> Result<(), String>,
) -> Result<(), String> {
    use std::os::windows::process::CommandExt;

    let msiexec = std::env::var("SYSTEMROOT")
        .map(|r| format!("{r}\\System32\\msiexec.exe"))
        .unwrap_or_else(|_| "msiexec.exe".into());
    log::info!(
        "updater: MSI leg running: {msiexec} {params} — the user sees ONE UAC prompt (a \
         per-machine package launched from a non-elevated app; /passive is the lowest UI \
         level at which Windows Installer can ask) and then a progress bar. This thread \
         WAITS, unlike the plugin's own installer, so a declined prompt leaves Spaceadom \
         running instead of killing it."
    );

    // REVIEW FIXES 2026-09-05 (H5) — mark the clean exit BEFORE msiexec starts,
    // because on the success path this process does not get another chance.
    //
    // PROBLEM 127's `util:CloseApplication` terminates us partway through the
    // install sequence. Until H2's `EndSessionMessage="yes"` is in the shipped
    // MSI that close is a `WM_CLOSE` — which this app answers by hiding to the
    // tray — followed by `TerminateProcess`. No `RunEvent`, no
    // `WM_ENDSESSION`, no `session_end::teardown`: an MSI self-update landing
    // inside the first 30 seconds of a launch left a `+1` on the boot counter
    // every single time.
    //
    // The cost of marking early is bounded and named: if the UAC prompt is
    // DECLINED (1602) this process survives with its increment already undone,
    // so a genuine crash later in the same 30-second window would go
    // unrecorded. That is the right way round — an update attempt is not a
    // startup crash, and the next launch counts normally.
    crate::safe_mode::note_clean_exit();

    // REVIEW FIXES 2026-09-05 (H1) — THE LAST CHECK, and it has to be the last
    // statement before the launch. Anything between this line and `raw_arg`
    // widens the window it exists to close.
    if let Err(why) = guard() {
        log::error!(
            "updater: REFUSING to run msiexec for {version} — {why}. Nothing was installed and \
             nothing was elevated. This is the check that stands between a file in a temporary \
             folder and an installer running with administrator rights; a failure here means \
             the package on disk is not the package this app verified (H1)."
        );
        if let Some(p) = cleanup {
            let _ = std::fs::remove_file(p);
        }
        return Err(
            "The update package failed its security check just before installing, so it was \
             NOT run. Nothing on this PC was changed."
                .into(),
        );
    }

    // `raw_arg`, not `arg`: msiexec does not parse its command line with
    // CommandLineToArgvW, and Rust's quoting would corrupt LAUNCHAPPARGS.
    let status = std::process::Command::new(&msiexec).raw_arg(params).status();

    // On the SUCCESS path we normally never get here: PROBLEM 127's
    // util:CloseApplication terminates this process partway through the
    // sequence, exactly as the NSIS leg's taskkill does, and the mutex is
    // released by the kernel before LaunchApplication runs after InstallFinalize.
    match status {
        Ok(s) if s.success() => {
            log::warn!(
                "updater: msiexec returned 0 while this process is STILL ALIVE — the \
                 package installed but util:CloseApplication did not close us, so this is \
                 the old build still running against new files on disk. Not exiting (a \
                 running old version beats no version); the next launch picks up {version}."
            );
            Ok(())
        }
        Ok(s) => {
            let code = s.code().unwrap_or(-1);
            let reason = msiexec_reason(code);
            if code == 1602 && MSI_DECLINE_EXPLAINED.swap(true, Ordering::Relaxed) {
                log::info!("updater: MSI update declined again (1602) — retrying tomorrow");
            } else {
                log::warn!(
                    "updater: msiexec exited {code} — {reason} No dialog, no toast, no \
                     badge: the next daily check simply tries again."
                );
            }
            if let Some(p) = cleanup {
                let _ = std::fs::remove_file(p);
            }
            Err(reason)
        }
        Err(e) => {
            log::warn!(
                "updater: could not start {msiexec} ({e}) — no update this time, the next \
                 daily check retries"
            );
            if let Some(p) = cleanup {
                let _ = std::fs::remove_file(p);
            }
            Err("Windows Installer could not be started on this PC. Nothing was changed.".into())
        }
    }
}

#[cfg(not(windows))]
fn install_msi(_bytes: &[u8], _version: &str, _dev_override: bool) -> Result<(), String> {
    log::warn!("updater: the MSI leg is Windows-only");
    Err("The MSI update path only exists on Windows.".into())
}

// ---------------------------------------------------------------------------
// "Updated to 1.0.X"
// ---------------------------------------------------------------------------

static UPDATE_NOTICE: Mutex<Option<String>> = Mutex::new(None);
/// True when the version change this launch represents went BACKWARDS — a
/// rollback, or a hand-install of an older build. `release_notes.rs` reads it
/// so the What's New panel can say "Rolled back to" instead of "Updated to";
/// the bare toast still says "Updated to" because its wording lives in the
/// frontend and is batch 2's to change.
static LAUNCH_WAS_ROLLBACK: AtomicBool = AtomicBool::new(false);

fn last_run_path() -> std::path::PathBuf {
    crate::startup::data_dir().join(LAST_RUN_FILE)
}

/// At startup. Decides whether this launch is the first of a new version and,
/// if so, holds the notice for the dashboard. Does NOT rewrite the file on an
/// upgrade — `take_update_notice` does, once the toast has been handed over.
pub fn record_launch_version(current: &str) {
    let path = last_run_path();
    let previous = std::fs::read_to_string(&path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    match previous {
        Some(prev) if prev == current => {}
        Some(prev) => {
            let backwards = cmp_versions(current, &prev) == std::cmp::Ordering::Less;
            LAUNCH_WAS_ROLLBACK.store(backwards, Ordering::Relaxed);
            log::info!(
                "updater: first launch of {current} after {prev} ({}) — the dashboard will \
                 show its one-time notice for {current} the next time it opens",
                if backwards { "a ROLLBACK — the version went backwards" } else { "an update" }
            );
            if let Ok(mut n) = UPDATE_NOTICE.lock() {
                *n = Some(current.to_string());
            }
        }
        None => {
            log::info!("updater: no last-run version on record — {current} recorded, no toast");
            write_last_run(&path, current);
        }
    }
}

fn write_last_run(path: &std::path::Path, version: &str) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Err(e) = std::fs::write(path, version) {
        log::warn!("updater: could not write {} ({e})", path.display());
    }
}

/// The dashboard's question. Answers once per upgrade, then records the
/// version so the next launch stays quiet.
pub fn take_update_notice() -> Option<String> {
    let notice = UPDATE_NOTICE.lock().ok().and_then(|mut n| n.take());
    if let Some(v) = &notice {
        write_last_run(&last_run_path(), v);
        log::info!("updater: 'Updated to {v}' handed to the dashboard and recorded");
    }
    notice
}

/// The same question WITHOUT consuming the answer. `release_notes.rs` needs to
/// know that this launch follows a version change, and it must not be the thing
/// that spends the one-shot notice — the dashboard's toast has to survive.
pub fn peek_update_notice() -> Option<String> {
    UPDATE_NOTICE.lock().ok().and_then(|n| n.clone())
}

/// Did the version change this launch represents go backwards?
pub fn update_was_a_rollback() -> bool {
    LAUNCH_WAS_ROLLBACK.load(Ordering::Relaxed)
}

// ===========================================================================
// PROBLEM 249 (1) — the MANUAL check
// ===========================================================================

/// What a check is doing, or did. One shape for the `update-status` EVENT and
/// for what `check_for_updates_now` RETURNS, so a UI written against one is
/// automatically right about the other.
///
/// `message` is always a finished sentence in plain English — never a code,
/// never a fragment. That is the contract that lets a UI handle an unknown
/// `state` by simply showing `message`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct UpdateStatus {
    /// One of the `STATE_*` constants above.
    pub state: &'static str,
    /// The version running right now.
    pub current: String,
    /// The newest version the manifest offers, when we got that far.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest: Option<String>,
    pub message: String,
    /// 0-100 while downloading; absent otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<u8>,
}

impl UpdateStatus {
    fn plain(state: &'static str, current: &str, message: impl Into<String>) -> Self {
        Self {
            state,
            current: current.to_string(),
            latest: None,
            message: message.into(),
            progress: None,
        }
    }
    fn err(current: &str, message: impl Into<String>) -> Self {
        Self::plain(STATE_ERROR, current, message)
    }
}

/// Publish a status to both webviews. Global `emit`, never `emit_to` — that
/// has never worked in this app (CLAUDE.md, window rules).
fn emit_always(app: &tauri::AppHandle, status: UpdateStatus) -> UpdateStatus {
    use tauri::Emitter;
    if let Err(e) = app.emit(EVENT_UPDATE_STATUS, status.clone()) {
        log::warn!("updater: could not emit {EVENT_UPDATE_STATUS} ({e})");
    }
    status
}

/// Publish only for a MANUAL check. Used for the outcomes that are answers to
/// a question the user asked — "you're on the latest", "that didn't work". The
/// daily check stays silent for these, so the tray label never rewrites itself
/// at whatever hour the timer fires.
fn emit_status(app: &tauri::AppHandle, status: UpdateStatus, manual: bool) -> UpdateStatus {
    if manual {
        return emit_always(app, status);
    }
    status
}

/// The rate limit, pure. `Some(sentence)` = refuse, and the sentence is what
/// the user reads.
///
/// Two separate reasons to refuse and they are not the same thing: a check
/// still RUNNING (a 6 MB download can take a minute) and a check that FINISHED
/// moments ago. Collapsing them would tell a user staring at a progress bar
/// that they should wait 30 seconds.
pub(crate) fn manual_gate(in_flight: bool, since_last: Option<Duration>, min_gap: Duration) -> Option<String> {
    if in_flight {
        return Some("Already checking for updates…".into());
    }
    match since_last {
        Some(d) if d < min_gap => {
            let wait = (min_gap - d).as_secs() + 1;
            Some(format!("Just checked. Try again in {wait} seconds."))
        }
        _ => None,
    }
}

/// **The manual "Check for updates now".**
///
/// Same `plan()`, same manifest, same installer routing as the daily check.
/// The two documented differences, both because this one is a user asking:
///
/// * **It runs, and installs, even when `auto_update` is `false`.** That flag
///   means "not behind my back". A click is not behind anybody's back, and a
///   report-only manual check would leave someone who set the flag once with no
///   route to a fix at all.
/// * **It ignores a rollback hold** and clears it, because the hold exists to
///   stop the app doing this on its own, not to stop the user.
///
/// On the NSIS leg **this never returns**: `update.install()` exits the
/// process. The UI must act on the `update-status` event carrying
/// `state: "installing"`, not on this promise resolving.
#[tauri::command]
pub async fn check_for_updates_now(app: tauri::AppHandle) -> UpdateStatus {
    run_manual_check(app).await
}

async fn run_manual_check(app: tauri::AppHandle) -> UpdateStatus {
    let current = app.package_info().version.to_string();

    let since = LAST_MANUAL_CHECK.lock().ok().and_then(|l| l.map(|t| t.elapsed()));
    if let Some(why) = manual_gate(MANUAL_IN_FLIGHT.load(Ordering::Relaxed), since, MANUAL_MIN_GAP) {
        log::info!("updater: manual check refused — {why}");
        return emit_always(&app, UpdateStatus::plain(STATE_BUSY, &current, why));
    }
    // The claim, and its release. `swap` rather than `store` so two callers
    // racing between the read above and here still cannot both proceed.
    if MANUAL_IN_FLIGHT.swap(true, Ordering::SeqCst) {
        let why = "Already checking for updates…";
        log::info!("updater: manual check refused — {why} (lost the race for the in-flight flag)");
        return emit_always(&app, UpdateStatus::plain(STATE_BUSY, &current, why));
    }
    if let Ok(mut l) = LAST_MANUAL_CHECK.lock() {
        *l = Some(std::time::Instant::now());
    }
    let out = manual_check_inner(&app, &current).await;
    MANUAL_IN_FLIGHT.store(false, Ordering::SeqCst);
    out
}

async fn manual_check_inner(app: &tauri::AppHandle, current: &str) -> UpdateStatus {
    // PROBLEM 250's gate applies to a MANUAL check too, and it is the FIRST
    // thing here for the same reason it is first in `run_check`: inside an MSIX
    // package there is nothing worth doing. A click does not change what the
    // installer would do — running a downloaded setup.exe from inside a package
    // leaves a SECOND, unpackaged Spaceadom beside this one with both hooking
    // the spacebar (PROBLEM 129). "The user asked for it" is a reason to
    // override a POLICY, never a reason to override a fact about the machine.
    if crate::packaged::is_packaged() {
        log::info!("updater: manual check refused — PACKAGED (MSIX); the Store owns updates here");
        return emit_always(
            app,
            UpdateStatus::plain(
                STATE_NOT_ELIGIBLE,
                current,
                "This copy came from the Microsoft Store, so the Store keeps it up to date. \
                 Check for updates in the Store app.",
            ),
        );
    }
    // PROBLEM 254 — and the same rule applies here as it does to the packaged
    // gate above: "the user asked for it" is a reason to override a POLICY,
    // never a reason to override a FACT about the machine. `auto_update:
    // false` is a policy and a click overrides it. "This copy was unzipped
    // rather than installed, so no installer of ours exists to re-run" is a
    // fact, and a click cannot make one appear.
    if crate::portable::is_portable() {
        log::info!("updater: manual check refused — PORTABLE; there is no installer to re-run");
        return emit_always(
            app,
            UpdateStatus::plain(
                STATE_NOT_ELIGIBLE,
                current,
                "This is the portable copy, so it can't update itself. \
                 Download the newest portable zip and unzip it over this folder.",
            ),
        );
    }

    emit_always(
        app,
        UpdateStatus::plain(STATE_CHECKING, current, "Checking for updates…"),
    );

    // A manual check overrides the rollback hold, and clears it: the user has
    // just asked for the newest version, so holding them back from tomorrow's
    // check as well would be the app arguing with them.
    if read_hold().is_some() {
        log::info!("updater: manual check — clearing the rollback hold, the user asked for this");
        clear_hold();
    }

    let kind = detect_install_kind();
    let (url, dev_override) = match plan(kind, read_override().as_deref()) {
        Plan::Skip { reason } => {
            log::info!("updater: manual check — no check: {reason}");
            return emit_always(
                app,
                UpdateStatus::plain(
                    STATE_NOT_ELIGIBLE,
                    current,
                    "This copy of Spaceadom can't update itself — it wasn't put here by the \
                     installer. Download the latest version from the Spaceadom releases page.",
                ),
            );
        }
        Plan::Check { url, dev_override } => (url, dev_override),
    };

    match check_and_install(app, &url, dev_override, kind, true).await {
        Ok(status) => status,
        Err(e) => {
            log::warn!("updater: manual check failed (offline / 404 / bad manifest / bad signature): {e}");
            emit_always(
                app,
                UpdateStatus::err(
                    current,
                    "Couldn't reach the update server. Check your internet connection and \
                     try again.",
                ),
            )
        }
    }
}

/// The tray's entry point. The tray menu callback runs on the MAIN thread, and
/// a manual check does a network round trip and possibly a 6 MB download — so
/// it gets its own thread, exactly as the daily check does. Never blocks the
/// menu.
pub fn manual_check_on_a_thread(app: tauri::AppHandle) {
    let spawned = std::thread::Builder::new()
        .name("st-updater-manual".into())
        .spawn(move || {
            tauri::async_runtime::block_on(run_manual_check(app));
        });
    if spawned.is_err() {
        log::error!("updater: could not spawn st-updater-manual — the manual check did not run");
    }
}

// ===========================================================================
// PROBLEM 249 (2) — the rollback archive
// ===========================================================================

/// One archived installer. `path` is absolute so the UI can show it and the
/// installer launch does not have to re-derive it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct RollbackTarget {
    pub version: String,
    pub path: String,
    /// `"nsis"` or `"msi"` — which installer would run.
    pub kind: &'static str,
}

/// The 24 h hold a rollback puts on the DAILY check.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct UpdateHold {
    /// Unix epoch milliseconds. The daily check skips while `now < until_ms`.
    pub until_ms: u64,
    /// The version we rolled back TO, so the log can name it.
    pub pinned_version: String,
    pub reason: String,
}

fn rollback_dir() -> std::path::PathBuf {
    crate::startup::data_dir().join(ROLLBACK_DIR)
}

fn hold_path() -> std::path::PathBuf {
    rollback_dir().join(HOLD_FILE)
}

pub(crate) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Pure. Does an existing hold still block the daily check?
pub(crate) fn hold_blocks_daily_check(now: u64, hold: Option<&UpdateHold>) -> bool {
    hold.is_some_and(|h| now < h.until_ms)
}

fn read_hold() -> Option<UpdateHold> {
    let text = std::fs::read_to_string(hold_path()).ok()?;
    match serde_json::from_str::<UpdateHold>(&text) {
        Ok(h) => Some(h),
        Err(e) => {
            log::warn!(
                "updater: {} does not parse ({e}) — treated as no hold, so a corrupt file can \
                 never freeze updates permanently",
                hold_path().display()
            );
            None
        }
    }
}

fn write_hold(hold: &UpdateHold) {
    let dir = rollback_dir();
    if let Err(e) = std::fs::create_dir_all(&dir) {
        log::warn!("updater: could not create {} ({e}) — no update hold written", dir.display());
        return;
    }
    match serde_json::to_string_pretty(hold) {
        Ok(text) => {
            if let Err(e) = std::fs::write(hold_path(), text) {
                log::warn!("updater: could not write {} ({e})", hold_path().display());
            } else {
                log::info!(
                    "updater: updates held until epoch-ms {} after rolling back to {}",
                    hold.until_ms,
                    hold.pinned_version
                );
            }
        }
        Err(e) => log::warn!("updater: could not serialise the update hold ({e})"),
    }
}

/// Removes `hold.json` only. Written by this feature, in this feature's own
/// directory — the one file here we are allowed to delete.
fn clear_hold() {
    let p = hold_path();
    if p.is_file() {
        if let Err(e) = std::fs::remove_file(&p) {
            log::warn!("updater: could not remove {} ({e})", p.display());
        }
    }
}

/// Compare two versions numerically, component by component. Pure.
///
/// Deliberately NOT the `semver` crate: it is in the tree only as
/// `tauri-plugin-updater`'s private dependency, and every version this app has
/// ever shipped is a plain `MAJOR.MINOR.PATCH` of decimal numbers. A build or
/// pre-release suffix (`1.0.101-rc.1`) is cut off and ignored, which orders it
/// EQUAL to `1.0.101` — the honest answer for a rollback decision, since the
/// archive would hold at most one of them anyway.
pub(crate) fn cmp_versions(a: &str, b: &str) -> std::cmp::Ordering {
    fn parts(v: &str) -> [u64; 4] {
        let core = v.trim().trim_start_matches('v');
        let core = core.split(['-', '+']).next().unwrap_or("");
        let mut out = [0u64; 4];
        for (i, seg) in core.split('.').take(4).enumerate() {
            out[i] = seg.trim().parse().unwrap_or(0);
        }
        out
    }
    parts(a).cmp(&parts(b))
}

/// The filename this feature writes for `version`/`kind`. Pure.
pub(crate) fn archive_name(version: &str, kind: InstallKind) -> Option<String> {
    let ext = match kind {
        InstallKind::Nsis => "exe",
        InstallKind::Msi => "msi",
        InstallKind::Unknown => return None,
    };
    Some(format!("{ARCHIVE_PREFIX}{version}{ARCHIVE_SUFFIX}.{ext}"))
}

/// The inverse. Pure, and STRICT on purpose: this is the predicate that decides
/// which files in the rollback directory this feature is allowed to delete, so
/// anything it does not recognise byte-for-byte as its own output is left
/// alone. A version must be non-empty and made only of digits and dots.
pub(crate) fn parse_archive_name(name: &str) -> Option<(String, InstallKind)> {
    let kind = if let Some(stem) = name.strip_suffix(".exe") {
        Some((stem, InstallKind::Nsis))
    } else {
        name.strip_suffix(".msi").map(|stem| (stem, InstallKind::Msi))
    };
    let (stem, kind) = kind?;
    let version = stem.strip_prefix(ARCHIVE_PREFIX)?.strip_suffix(ARCHIVE_SUFFIX)?;
    if version.is_empty()
        || !version.chars().all(|c| c.is_ascii_digit() || c == '.')
        || !version.chars().any(|c| c.is_ascii_digit())
    {
        return None;
    }
    Some((version.to_string(), kind))
}

// ---------------------------------------------------------------------------
// REVIEW FIXES 2026-09-05 (H1) — integrity.
//
// **The hole this closes.** `archive_installer` wrote 6 MB of installer into
// `%APPDATA%\Spaceadom\rollback\` and nothing ever looked at it again.
// `rollback_to_previous` ran that file — an NSIS installer that writes into
// `%LOCALAPPDATA%`, or an `.msi` that runs behind a UAC prompt the user is
// about to accept BECAUSE the app asked for it. `%APPDATA%` is writable by
// anything running as this user, and by anything that has already got a
// foothold as this user. Replacing one file there, with no name to guess and
// no signature to forge, turned "go back to the previous version" into
// "execute this, elevated, with the owner's blessing".
//
// The bytes ARE signed — `tauri-plugin-updater` verifies them against the
// public key baked into this exe before it ever hands them over
// (`updater.rs::verify_signature`, 2.11.0) — but the signature was thrown away
// the moment the download finished. So: keep it, and check it again.
// ---------------------------------------------------------------------------

/// The public key an archived installer is checked against: the SAME one every
/// download is checked against, read straight out of `tauri.conf.json` at
/// runtime (`plugins.updater.pubkey`) rather than copied into a Rust constant
/// that could drift away from it.
fn updater_pubkey(app: &tauri::AppHandle) -> Option<String> {
    app.config()
        .plugins
        .0
        .get("updater")?
        .get("pubkey")?
        .as_str()
        .map(str::to_string)
}

/// Verify `data` against a base64 minisign `signature` and a base64 minisign
/// `pubkey`. **A transcription of `tauri-plugin-updater 2.11.0`'s private
/// `verify_signature`**, using the same `minisign-verify` crate the plugin
/// uses, because that function is not public and cannot be asked about a file
/// the plugin did not download.
///
/// Pure — no disk, no network — so the "a planted file is refused" case has a
/// unit test.
pub(crate) fn verify_minisign(
    data: &[u8],
    signature_b64: &str,
    pubkey_b64: &str,
) -> Result<(), String> {
    use base64::Engine;
    let decode = |s: &str, what: &str| -> Result<String, String> {
        let raw = base64::engine::general_purpose::STANDARD
            .decode(s.trim())
            .map_err(|e| format!("the {what} is not valid base64 ({e})"))?;
        String::from_utf8(raw).map_err(|_| format!("the {what} is not valid UTF-8"))
    };
    let key = minisign_verify::PublicKey::decode(&decode(pubkey_b64, "public key")?)
        .map_err(|e| format!("the built-in public key could not be decoded ({e:?})"))?;
    let sig = minisign_verify::Signature::decode(&decode(signature_b64, "signature")?)
        .map_err(|e| format!("the signature could not be decoded ({e:?})"))?;
    // `true` for `allow_legacy`, exactly as the plugin passes it — Tauri's
    // signer emits non-prehashed signatures, and refusing them here would
    // reject every installer the plugin itself accepts.
    key.verify(data, &sig, true)
        .map_err(|e| format!("the signature does not match the file ({e:?})"))
}

/// `<installer>.sig`.
fn sig_path_for(installer: &std::path::Path) -> std::path::PathBuf {
    let mut s = installer.as_os_str().to_os_string();
    s.push(ARCHIVE_SIG_EXT);
    std::path::PathBuf::from(s)
}

/// Re-verify an archived installer against its stored signature.
///
/// `Err(sentence)` for every reason an archived file must not be run: no
/// signature file, an unreadable one, an unreadable installer, or — the one
/// that matters — bytes that the signature does not cover.
fn verify_archived_installer(installer: &std::path::Path, pubkey: &str) -> Result<(), String> {
    let sig_path = sig_path_for(installer);
    let signature = std::fs::read_to_string(&sig_path).map_err(|e| {
        format!(
            "there is no usable signature beside it ({} — {e}). Installers archived before \
             this check existed have none, and they are refused rather than trusted.",
            sig_path.display()
        )
    })?;
    let bytes = std::fs::read(installer)
        .map_err(|e| format!("the archived installer could not be read ({e})"))?;
    verify_minisign(&bytes, signature.trim(), pubkey)
}

/// Lowercase hex SHA-256. Pure.
pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .fold(String::with_capacity(64), |mut acc, b| {
            use std::fmt::Write;
            let _ = write!(acc, "{b:02x}");
            acc
        })
}

/// A per-call directory suffix for the `%TEMP%` staging directory.
///
/// `RandomState` is seeded from the operating system, so the name cannot be
/// predicted and pre-created by another process running as this user — which
/// is the whole point of not writing to a fixed `%TEMP%\Spaceadom-1.0.X-installer.msi`
/// that anything could have put there first, or could swap between our write
/// and msiexec's read.
fn temp_dir_suffix() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u64(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0),
    );
    h.write_u32(std::process::id());
    format!("{:016x}", h.finish())
}

/// One entry in the archive, as the pure retention logic sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ArchiveEntry {
    pub version: String,
    pub file: String,
    pub kind: InstallKind,
}

/// What the archive should look like after a prune. Pure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Retention {
    /// Filenames to keep, at most two.
    pub keep: Vec<String>,
    /// Filenames to delete. Only ever names that `parse_archive_name` accepted.
    pub delete: Vec<String>,
    /// The rollback target, if there is one: the NEWEST entry older than the
    /// running version.
    pub target: Option<ArchiveEntry>,
}

/// **The retention rule, whole, in one pure function.**
///
/// Keep exactly two things and no more:
///
/// * the entry whose version IS the running version — the SEED. When this
///   machine later updates away from here, that file is the only way back.
/// * the newest entry OLDER than the running version — the TARGET, what
///   `rollback_to_previous` runs.
///
/// Everything else goes, including anything NEWER than the running version: a
/// file like that is the leftover of an update that was downloaded and then did
/// not take (a declined UAC, a failed install). It is neither a seed nor a way
/// back, the plugin always re-downloads rather than reusing a local file, so
/// keeping it buys nothing and costs 6 MB.
///
/// A mismatched `kind` is never a target — an `.msi` archived by a per-machine
/// copy must not be run by a per-user NSIS copy and vice versa (PROBLEM
/// 129/244) — but it is not deleted either, because the two copies share
/// `%APPDATA%` and the other one's seed is not ours to throw away.
pub(crate) fn retention(running: &str, kind: InstallKind, mut entries: Vec<ArchiveEntry>) -> Retention {
    use std::cmp::Ordering as O;
    // Newest first, and a stable tiebreak on the filename so the result never
    // depends on the order the directory happened to be read in.
    entries.sort_by(|a, b| cmp_versions(&b.version, &a.version).then_with(|| a.file.cmp(&b.file)));

    let mut keep: Vec<String> = Vec::new();
    let mut delete: Vec<String> = Vec::new();
    let mut target: Option<ArchiveEntry> = None;
    let mut seeded = false;

    for e in entries {
        let ours = e.kind == kind;
        match cmp_versions(&e.version, running) {
            O::Equal if ours && !seeded => {
                seeded = true;
                keep.push(e.file);
            }
            O::Less if ours && target.is_none() => {
                keep.push(e.file.clone());
                target = Some(e);
            }
            _ if !ours => keep.push(e.file),
            _ => delete.push(e.file),
        }
    }
    Retention { keep, delete, target }
}

/// Read the archive directory into entries. Anything the strict parser does not
/// recognise is skipped entirely — it is never listed and never deleted.
fn read_archive() -> Vec<ArchiveEntry> {
    let dir = rollback_dir();
    let Ok(rd) = std::fs::read_dir(&dir) else { return Vec::new() };
    let mut out = Vec::new();
    for entry in rd.flatten() {
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if let Some((version, kind)) = parse_archive_name(&name) {
            out.push(ArchiveEntry { version, file: name, kind });
        }
    }
    out
}

/// Write the installer we are about to run into the archive. Called from
/// `check_and_install` BEFORE the install, because on the NSIS leg there is no
/// "after".
///
/// **REVIEW FIXES 2026-09-05 (H1) — the detached signature is archived with
/// it.** `signature` is `Update::signature`, the base64 minisign signature the
/// plugin took from the manifest and has just checked these very bytes
/// against; it is a public field, so nothing has to be re-downloaded.
///
/// **An installer with no signature beside it is not archived at all.** Half an
/// archive is worse than none: `rollback_available` would list a version the
/// user can see and cannot use, and the difference between "refused because it
/// is unsigned" and "refused because it was tampered with" would be invisible.
fn archive_installer(bytes: &[u8], version: &str, kind: InstallKind, signature: &str) {
    let Some(name) = archive_name(version, kind) else {
        log::info!("updater: install kind {kind:?} has no installer to archive — no rollback copy");
        return;
    };
    let dir = rollback_dir();
    if let Err(e) = std::fs::create_dir_all(&dir) {
        log::warn!(
            "updater: could not create {} ({e}) — this update installs normally but leaves no \
             way back",
            dir.display()
        );
        return;
    }
    let path = dir.join(&name);
    if let Err(e) = std::fs::write(&path, bytes) {
        log::warn!(
            "updater: could not archive {name} to {} ({e}) — the update still installs, there \
             is simply no rollback copy of it",
            path.display()
        );
        return;
    }
    let sig_path = sig_path_for(&path);
    if let Err(e) = std::fs::write(&sig_path, signature.trim()) {
        log::warn!(
            "updater: archived {name} but could NOT write its signature to {} ({e}). Removing \
             the installer again: an archived installer with no signature beside it is refused \
             at rollback time anyway (H1), and leaving 6 MB of unusable file behind would \
             advertise a way back that does not work. The update itself is unaffected.",
            sig_path.display()
        );
        // Removing a file this function created seconds ago, by the name it
        // chose itself — the same rule the prune obeys.
        let _ = std::fs::remove_file(&path);
        return;
    }
    log::info!(
        "updater: archived the {} bytes of {name} to {}, with its minisign signature beside it \
         as {} — after the next update this is the way back to {version}, and it is re-verified \
         against the built-in public key before it is ever run (PROBLEM 249 + H1)",
        bytes.len(),
        path.display(),
        sig_path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
    );
}

/// Apply `retention` to the real directory. Runs once per launch, on the
/// `st-updater` thread, past the settle.
fn prune_rollback_archive(running: &str) {
    let kind = detect_install_kind();
    if kind == InstallKind::Unknown {
        log::info!("updater: install kind Unknown — the rollback archive is left completely alone");
        return;
    }
    let entries = read_archive();
    if entries.is_empty() {
        log::info!("updater: no archived installers yet — rollback is unavailable until the second update");
        return;
    }
    let plan = retention(running, kind, entries);
    for file in &plan.delete {
        let p = rollback_dir().join(file);
        match std::fs::remove_file(&p) {
            Ok(()) => log::info!("updater: pruned {file} from the rollback archive"),
            Err(e) => log::warn!("updater: could not prune {} ({e})", p.display()),
        }
        // REVIEW FIXES 2026-09-05 (H1) — the signature is the installer's
        // companion, never an entry of its own: `parse_archive_name` refuses
        // `.sig` precisely so this feature can never be argued into deleting
        // one on its own account. It goes when its installer goes.
        let sig = sig_path_for(&p);
        if sig.is_file() {
            match std::fs::remove_file(&sig) {
                Ok(()) => log::info!("updater: pruned {file}{ARCHIVE_SIG_EXT} with it"),
                Err(e) => log::warn!("updater: could not prune {} ({e})", sig.display()),
            }
        }
    }
    log::info!(
        "updater: rollback archive on {running} — keeping {:?}, pruned {:?}, way back: {}",
        plan.keep,
        plan.delete,
        plan.target.as_ref().map(|t| t.version.as_str()).unwrap_or("none")
    );
}

/// **Is there a previous version to go back to?** `None` on the very first
/// update (the copy we left was hand-installed and its installer was never
/// ours), on a dev build, and whenever the archive holds nothing older than
/// what is running.
#[tauri::command]
pub fn rollback_available(app: tauri::AppHandle) -> Option<RollbackTarget> {
    // PROBLEM 250 — a packaged copy has no installer of its own to re-run, and
    // running an unpackaged one beside it is PROBLEM 129.
    if crate::packaged::is_packaged() {
        return None;
    }
    // PROBLEM 254 — a portable copy. `detect_install_kind()` already returns
    // `Unknown` for one and the next line would return `None` anyway, so this
    // gate changes no behaviour; it exists for the LOG LINE. The two "no
    // rollback here" cases are answered by different facts (no installer of
    // ours vs. no install at all) and a silent shared `None` makes them
    // indistinguishable in a bug report.
    if crate::portable::is_portable() {
        log::info!(
            "updater: no rollback offered — PORTABLE. Nothing was installed, so no previous \
             installer was ever archived; keep the old zip if you want a way back \
             (PROBLEM 254)."
        );
        return None;
    }
    let kind = detect_install_kind();
    if kind == InstallKind::Unknown {
        return None;
    }
    let running = app.package_info().version.to_string();
    let target = retention(&running, kind, read_archive()).target?;
    let path = rollback_dir().join(&target.file);
    if !path.is_file() {
        return None;
    }

    // REVIEW FIXES 2026-09-05 (H1) — NO SIGNATURE, NO ROLLBACK. The button is
    // not offered at all unless this exact file still verifies against the
    // public key in tauri.conf.json.
    //
    // Refused here rather than only at `rollback_to_previous`, because the two
    // failures a user can tell apart are "there is nothing to go back to" and
    // "there is something and it is wrong". A greyed-out button plus a loud log
    // line is the first; a button that fails when pressed teaches the user to
    // press it again.
    let Some(pubkey) = updater_pubkey(&app) else {
        log::error!(
            "updater: NO ROLLBACK — plugins.updater.pubkey is missing from this build's \
             configuration, so an archived installer cannot be verified against anything. \
             Refusing to offer a rollback rather than running an unverified installer \
             elevated (H1)."
        );
        return None;
    };
    if let Err(why) = verify_archived_installer(&path, &pubkey) {
        log::error!(
            "updater: NO ROLLBACK — the archived installer {} FAILED verification: {why} \
             This file is not offered and will not be run. It is left on disk untouched so it \
             can be inspected; delete it by hand if you want the rollback offer back. A \
             mismatch here means the bytes in %APPDATA% are not the bytes this app downloaded \
             and had signature-checked (H1).",
            path.display()
        );
        return None;
    }
    log::info!(
        "updater: rollback target {} verified against the built-in public key — offering it",
        target.version
    );

    Some(RollbackTarget {
        version: target.version,
        path: path.to_string_lossy().to_string(),
        kind: match target.kind {
            InstallKind::Msi => "msi",
            _ => "nsis",
        },
    })
}

/// **Go back to the previous version.**
///
/// Runs the archived installer exactly the way the updater runs a new one —
/// NSIS `/S /UPDATE /R /ARGS --autostart`, MSI through the PROBLEM 246 msiexec
/// command with the same one-UAC-prompt semantics — after `stop_hook()`, so the
/// incoming instance never shares the keyboard with this one.
///
/// A 24 h hold is written FIRST, before anything irreversible: if the process
/// is killed mid-install (which is the normal NSIS ending), the hold is already
/// on disk and tomorrow's daily check will not simply put the user back on the
/// version they just left.
///
/// **On the NSIS leg this does not return** — the process exits. `Err` is only
/// ever the "nothing happened" case.
#[tauri::command]
pub fn rollback_to_previous(app: tauri::AppHandle) -> Result<String, String> {
    let running = app.package_info().version.to_string();
    let Some(target) = rollback_available(app.clone()) else {
        return Err("There's no previous version saved on this PC to go back to.".into());
    };
    let path = std::path::PathBuf::from(&target.path);

    // REVIEW FIXES 2026-09-05 (H1) — verified AGAIN, here, immediately before
    // anything irreversible.
    //
    // `rollback_available` above already checked it, and this is not a
    // duplicate: between that check and this line the file could have been
    // swapped, and the whole point of the attack is that the swap happens at
    // the last possible moment. The cost is one 6 MB read and an ed25519
    // verify — milliseconds — on a path the user reaches by pressing a button.
    // The MSI leg narrows the window further still with a third check inside
    // `run_msiexec`, right before `raw_arg`.
    let pubkey = updater_pubkey(&app).ok_or_else(|| {
        log::error!(
            "updater: ROLLBACK REFUSED — plugins.updater.pubkey is missing from this build's \
             configuration; nothing can be verified, so nothing is run (H1)."
        );
        "This copy of Spaceadom has no update signing key configured, so a saved installer \
         cannot be checked before it runs. Nothing was changed."
            .to_string()
    })?;
    if let Err(why) = verify_archived_installer(&path, &pubkey) {
        log::error!(
            "updater: ROLLBACK REFUSED — {} FAILED verification immediately before launch: \
             {why} Nothing was run and no update hold was written. The file is left in place \
             for inspection (H1).",
            path.display()
        );
        return Err(
            "The saved installer for the previous version failed its security check, so it \
             was NOT run. Nothing on this PC was changed."
                .into(),
        );
    }

    write_hold(&UpdateHold {
        until_ms: now_ms() + ROLLBACK_HOLD.as_millis() as u64,
        pinned_version: target.version.clone(),
        reason: format!("rolled back from {running} to {} — automatic updates are paused for 24 hours, \
                         and 'Check for updates' overrides that at any time", target.version),
    });

    log::warn!(
        "updater: ROLLBACK — {running} → {} using {} ({}). Stopping the hook first; the \
         installer takes over and relaunches with --autostart (PROBLEM 249).",
        target.version,
        path.display(),
        target.kind
    );

    let outcome = match target.kind {
        // **The MSI leg deliberately does NOT call `stop_hook()`**, and that is
        // the difference between the two legs rather than an omission.
        //
        // `run_msiexec` WAITS, and its whole reason for existing is that a
        // declined UAC prompt has to leave Spaceadom running (module doc,
        // PROBLEM 246). Stopping the hook first would make "running" a lie:
        // the app would still be in the tray with the spacebar silently doing
        // nothing, and nothing on screen would say why. On the success path
        // the hook needs no stopping — PROBLEM 127's `util:CloseApplication`
        // terminates this whole process partway through the sequence, exactly
        // as it does for an ordinary MSI update.
        "msi" => {
            let params = msi_parameters(&target.path);
            #[cfg(windows)]
            {
                // No cleanup path: the package IS the archive and must survive
                // a declined prompt so the user can try again.
                //
                // REVIEW FIXES 2026-09-05 (H1) — the guard is a THIRD signature
                // check, run as the statement before `raw_arg`. The archive
                // lives in `%APPDATA%`, which anything running as this user can
                // write; this is what stops the last-moment swap.
                let archive = path.clone();
                let key = pubkey.clone();
                run_msiexec(
                    &params,
                    &target.version,
                    None,
                    &move || verify_archived_installer(&archive, &key),
                )
                .map(|()| format!("Rolled back to {}.", target.version))
            }
            #[cfg(not(windows))]
            {
                let _ = params;
                Err("Rolling back is Windows-only.".into())
            }
        }
        _ => run_nsis_installer(&app, &path, &target.version),
    };
    // Only reached when NOTHING happened — the NSIS leg exits the process and
    // the MSI leg is normally killed mid-wait. A hold that outlived a rollback
    // which never took would pause updates for a day for no reason at all.
    if outcome.is_err() {
        log::info!("updater: the rollback did not happen — lifting the 24 h hold it had claimed");
        clear_hold();
    }
    outcome
}

/// Launch an archived NSIS installer and get out of its way.
///
/// The ORDER is the whole of PROBLEM 233: stop the hook and run Tauri's exit
/// cleanup, THEN start the installer, THEN exit — never `AppHandle::restart()`,
/// which spawns while this process still holds the single-instance mutex, and
/// never a spawn that outlives us holding the keyboard. `installer-hooks.nsh`'s
/// PREINSTALL taskkills anything of ours still standing, and `/R` starts the
/// new exe seconds after the kernel released the mutex.
#[cfg(windows)]
fn run_nsis_installer(
    app: &tauri::AppHandle,
    path: &std::path::Path,
    version: &str,
) -> Result<String, String> {
    use std::os::windows::process::CommandExt;
    /// CREATE_NO_WINDOW — `/S` draws nothing anyway; this stops a console
    /// flashing up if the installer ever decides it wants one.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    // The same switches the plugin composes for an update: silent, flagged as
    // an upgrade (no new shortcuts, no data removal), relaunch when done, and
    // the relaunch argv that makes the returning instance logon-shaped.
    //
    // **The spawn comes BEFORE the teardown**, and that is a deliberate
    // departure from `tauri-plugin-updater`, which runs its `on_before_exit`
    // and only then calls `ShellExecuteW` — so a launch that fails there leaves
    // the app alive with a dead keyboard hook and nothing on screen to say so.
    // PROBLEM 233's ordering rule is about the NEW instance never overlapping
    // the old one's hook, and that is still satisfied by a wide margin: the
    // installer's PREINSTALL taskkills us and waits 1.5 s, then copies files,
    // and only then does `/R` start anything. Stopping the hook microseconds
    // after a SUCCESSFUL spawn is seconds early.
    let spawned = std::process::Command::new(path)
        .args(["/S", "/UPDATE", "/R", "/ARGS", "--autostart"])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
    match spawned {
        Ok(_) => {
            log::warn!(
                "updater: rollback installer started for {version} — stopping the keyboard \
                 hook, running Tauri's exit cleanup, and exiting now so the single-instance \
                 mutex is released before /R relaunches us. Config needs no flush: every \
                 change is already on disk."
            );
            crate::hook::stop_hook();
            // REVIEW FIXES 2026-09-05 (H5) — same reason as the update leg: this
            // `process::exit(0)` produces no `RunEvent` and no `WM_ENDSESSION`,
            // so without this line a rollback started within 30 seconds of
            // launch would be recorded as a startup crash. Marked here, after
            // the installer has actually spawned, so the `Err` arm below (where
            // nothing happened and the app carries on) leaves the counter alone.
            crate::safe_mode::note_clean_exit();
            app.cleanup_before_exit();
            std::process::exit(0);
        }
        Err(e) => {
            log::error!(
                "updater: could not start the rollback installer {} ({e}) — the hook was \
                 never stopped, so Spaceadom is untouched and still working",
                path.display()
            );
            Err("The saved installer for the previous version could not be started.".into())
        }
    }
}

#[cfg(not(windows))]
fn run_nsis_installer(
    _app: &tauri::AppHandle,
    _path: &std::path::Path,
    _version: &str,
) -> Result<String, String> {
    Err("Rolling back is Windows-only.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    const NSIS_DIR: &str = r"C:\Users\beamu\AppData\Local\Spaceadom";
    const PF_DIR: &str = r"C:\Program Files\Spaceadom";

    #[test]
    fn uninstall_exe_beside_the_exe_means_nsis() {
        assert_eq!(classify_install(true, None, NSIS_DIR), InstallKind::Nsis);
    }

    /// PROBLEM 244's shape: the MSI registered the NSIS folder as its own. The
    /// files were written by NSIS, so NSIS is what may replace them — sending
    /// the .msi here is exactly the accident this module exists to prevent.
    #[test]
    fn nsis_folder_with_an_msi_registration_pointing_at_it_is_still_nsis() {
        assert_eq!(classify_install(true, Some(NSIS_DIR), NSIS_DIR), InstallKind::Nsis);
    }

    #[test]
    fn msi_registration_for_this_folder_and_no_uninstall_exe_means_msi() {
        assert_eq!(classify_install(false, Some(PF_DIR), PF_DIR), InstallKind::Msi);
        // Registry values come quoted, trailing-slashed and in any case.
        assert_eq!(
            classify_install(false, Some("\"c:\\program files\\spaceadom\\\""), PF_DIR),
            InstallKind::Msi
        );
    }

    #[test]
    fn msi_registration_for_some_other_folder_is_unknown() {
        assert_eq!(classify_install(false, Some(PF_DIR), NSIS_DIR), InstallKind::Unknown);
        assert_eq!(classify_install(false, Some(""), NSIS_DIR), InstallKind::Unknown);
    }

    #[test]
    fn no_evidence_at_all_is_unknown_and_never_updated() {
        assert_eq!(classify_install(false, None, r"D:\repo\target\release"), InstallKind::Unknown);
        assert!(matches!(plan(InstallKind::Unknown, None), Plan::Skip { .. }));
        // The override does not lift the kind gate.
        assert!(matches!(
            plan(InstallKind::Unknown, Some("https://127.0.0.1:8765/latest.json")),
            Plan::Skip { .. }
        ));
    }

    #[test]
    fn nsis_checks_the_nsis_manifest_on_github() {
        assert_eq!(
            plan(InstallKind::Nsis, None),
            Plan::Check { url: NSIS_MANIFEST.into(), dev_override: false }
        );
        assert!(NSIS_MANIFEST.ends_with("/latest.json"));
        assert!(NSIS_MANIFEST.starts_with("https://github.com/nur-arpon/Spaceadom/"));
    }

    #[test]
    fn the_override_file_replaces_the_url_and_marks_the_check_as_dev() {
        assert_eq!(
            plan(InstallKind::Nsis, Some("  https://127.0.0.1:8765/latest.json \n")),
            Plan::Check { url: "https://127.0.0.1:8765/latest.json".into(), dev_override: true }
        );
        // An empty file is the same as no file.
        assert_eq!(
            plan(InstallKind::Nsis, Some("   ")),
            Plan::Check { url: NSIS_MANIFEST.into(), dev_override: false }
        );
    }

    /// The MSI leg is ON (PROBLEM 246) and must read the MSI manifest — never
    /// `latest.json`, which points at the setup.exe. Feeding an MSI install the
    /// NSIS installer is PROBLEM 129 from the other direction.
    #[test]
    fn msi_checks_the_msi_manifest_and_never_the_nsis_one() {
        assert!(MSI_AUTO_UPDATE, "the owner turned the MSI leg on — PROBLEM 246");
        assert_eq!(
            plan(InstallKind::Msi, None),
            Plan::Check { url: MSI_MANIFEST.into(), dev_override: false }
        );
        assert!(MSI_MANIFEST.ends_with("/latest-msi.json"));
        assert_ne!(MSI_MANIFEST, NSIS_MANIFEST);
    }

    /// The exact msiexec command line, asserted whole. This is the artefact
    /// that stands in for a live install on a machine where installing the
    /// .msi is forbidden (PROBLEM 244), so it is checked literally.
    #[test]
    fn the_msi_command_line_is_exactly_what_the_docs_and_the_dry_run_say() {
        assert_eq!(
            msi_parameters(r"C:\Users\beamu\AppData\Local\Temp\Spaceadom-1.0.101-installer.msi"),
            "/i \"C:\\Users\\beamu\\AppData\\Local\\Temp\\Spaceadom-1.0.101-installer.msi\" \
             /passive /norestart REBOOT=ReallySuppress AUTOLAUNCHAPP=True \
             LAUNCHAPPARGS=\"--autostart\""
        );
    }

    /// Each switch is load-bearing and none may quietly disappear.
    #[test]
    fn the_msi_command_line_asks_for_consent_relaunches_and_never_reboots() {
        let p = msi_parameters(r"C:\tmp\x.msi");
        // /passive, NOT /quiet: at UI level none the Windows Installer service
        // has no way to raise the UAC dialog, so a per-machine install started
        // from this non-elevated app just fails. Measured, see MSI_SWITCHES.
        assert!(p.contains("/passive"), "{p}");
        assert!(!p.contains("/quiet") && !p.contains("/qn"), "{p}");
        // Never reboot a user's machine for a background update. The property
        // is the binding half; the switch is the echo.
        assert!(p.contains("REBOOT=ReallySuppress"), "{p}");
        assert!(p.contains("/norestart"), "{p}");
        // The relaunch, consumed by wix/main.wxs's LaunchApplication action.
        assert!(p.contains("AUTOLAUNCHAPP=True"), "{p}");
        assert!(p.contains("LAUNCHAPPARGS=\"--autostart\""), "{p}");
        // The package path is quoted — Program Files and %TEMP% both contain
        // spaces on a normal machine, and msiexec would otherwise cut the
        // argument at the first one.
        assert!(p.starts_with("/i \"") && p.contains("x.msi\""), "{p}");
    }

    /// PROBLEM 129/244's real-world shape: BOTH installers present at once, in
    /// their own proper folders. The verdict must depend only on WHICH COPY IS
    /// RUNNING, because that is the copy whose files are about to be replaced.
    #[test]
    fn both_installed_at_once_the_running_copy_decides_msi_direction() {
        // Running the Program Files copy: no uninstall.exe (WiX writes none)
        // and the MSI product is registered for this very folder.
        assert_eq!(
            classify_install(false, Some(PF_DIR), PF_DIR),
            InstallKind::Msi,
            "the Program Files copy must update itself with the .msi"
        );
        assert_eq!(
            plan(classify_install(false, Some(PF_DIR), PF_DIR), None),
            Plan::Check { url: MSI_MANIFEST.into(), dev_override: false }
        );
    }

    #[test]
    fn both_installed_at_once_the_running_copy_decides_nsis_direction() {
        // Running the %LOCALAPPDATA% copy while that same MSI product is
        // registered for Program Files. uninstall.exe is beside this exe, so
        // NSIS put these files here and NSIS is what may replace them.
        assert_eq!(
            classify_install(true, Some(PF_DIR), NSIS_DIR),
            InstallKind::Nsis,
            "the per-user copy must never be fed the .msi — PROBLEM 129"
        );
        assert_eq!(
            plan(classify_install(true, Some(PF_DIR), NSIS_DIR), None),
            Plan::Check { url: NSIS_MANIFEST.into(), dev_override: false }
        );
    }

    #[test]
    fn same_dir_is_case_insensitive_and_ignores_quotes_and_trailing_separators() {
        assert!(same_dir("\"C:\\Program Files\\Spaceadom\\\"", "c:\\program files\\spaceadom"));
        assert!(!same_dir("", ""));
        assert!(!same_dir(PF_DIR, NSIS_DIR));
    }

    // -----------------------------------------------------------------------
    // PROBLEM 249
    // -----------------------------------------------------------------------

    fn nsis(version: &str) -> ArchiveEntry {
        ArchiveEntry {
            version: version.into(),
            file: archive_name(version, InstallKind::Nsis).unwrap(),
            kind: InstallKind::Nsis,
        }
    }

    #[test]
    fn versions_compare_numerically_not_as_text() {
        use std::cmp::Ordering::*;
        // The one that matters: 1.0.9 vs 1.0.100. String order says 1.0.9 is
        // the newer of the two, and a rollback aimed by string order would try
        // to "go back" to a version that is ahead.
        assert_eq!(cmp_versions("1.0.100", "1.0.9"), Greater);
        assert_eq!(cmp_versions("1.0.9", "1.0.100"), Less);
        assert_eq!(cmp_versions("1.0.100", "1.0.100"), Equal);
        assert_eq!(cmp_versions("2.0.0", "1.99.99"), Greater);
        // Leading v, whitespace, a short version, and a suffix that is cut off.
        assert_eq!(cmp_versions("v1.0.100", " 1.0.100 "), Equal);
        assert_eq!(cmp_versions("1.0", "1.0.0"), Equal);
        assert_eq!(cmp_versions("1.0.101-rc.1", "1.0.101"), Equal);
        // Garbage sorts as 0.0.0.0 rather than panicking or ordering randomly.
        assert_eq!(cmp_versions("", "0.0.0"), Equal);
        assert_eq!(cmp_versions("not-a-version", "1.0.0"), Less);
    }

    #[test]
    fn an_archive_name_round_trips_and_the_parser_refuses_everything_else() {
        assert_eq!(
            archive_name("1.0.101", InstallKind::Nsis).as_deref(),
            Some("Spaceadom-1.0.101-installer.exe")
        );
        assert_eq!(
            archive_name("1.0.101", InstallKind::Msi).as_deref(),
            Some("Spaceadom-1.0.101-installer.msi")
        );
        assert_eq!(archive_name("1.0.101", InstallKind::Unknown), None);
        assert_eq!(
            parse_archive_name("Spaceadom-1.0.101-installer.exe"),
            Some(("1.0.101".into(), InstallKind::Nsis))
        );
        assert_eq!(
            parse_archive_name("Spaceadom-1.0.101-installer.msi"),
            Some(("1.0.101".into(), InstallKind::Msi))
        );
    }

    /// `parse_archive_name` is the predicate that decides what this feature is
    /// allowed to DELETE, so every one of these must come back `None`. The rule
    /// it enforces: if we did not write that exact name, we do not touch it.
    #[test]
    fn the_delete_predicate_refuses_anything_this_feature_did_not_write() {
        for name in [
            "hold.json",
            "config.json",
            "debug.log",
            "Spaceadom-installer.exe",           // no version at all
            "Spaceadom--installer.exe",          // empty version
            "Spaceadom-1.0.101-installer.txt",   // not an installer extension
            "Spaceadom-1.0.101-installer",       // no extension
            "spaceadom-1.0.101-installer.exe",   // wrong case on the prefix
            "Spaceadom-1.0.101-setup.exe",       // wrong suffix
            "Spaceadom-..-installer.exe",        // dots only, no digit
            "Spaceadom-../../evil-installer.exe", // traversal
            "Spaceadom-1.0.101 (copy)-installer.exe",
            "PROJECT_STATUS.md.tmp",             // the 2026-09-04 lesson
        ] {
            assert_eq!(parse_archive_name(name), None, "must never be deletable: {name}");
        }
    }

    /// **The first auto-update leaves no way back, and must not pretend to.**
    /// The copy being left was installed by hand; its installer was never ours.
    #[test]
    fn the_first_update_has_no_rollback_target() {
        // Running 1.0.101, which we installed ourselves and archived. There is
        // nothing older, because 1.0.100 came from a double-clicked setup.exe.
        let r = retention("1.0.101", InstallKind::Nsis, vec![nsis("1.0.101")]);
        assert_eq!(r.target, None);
        // ...and the seed is kept, because it is the way back from 1.0.102.
        assert_eq!(r.keep, vec!["Spaceadom-1.0.101-installer.exe"]);
        assert!(r.delete.is_empty());
        // An empty archive is the same answer with nothing kept.
        let r = retention("1.0.101", InstallKind::Nsis, vec![]);
        assert_eq!(r, Retention { keep: vec![], delete: vec![], target: None });
    }

    /// From the SECOND update onward there is always a way back, and it is
    /// always exactly one version. This is the case the two-slot archive exists
    /// for — a single slot alternates between having a target and not.
    #[test]
    fn from_the_second_update_onward_the_target_is_the_version_just_left() {
        let r = retention(
            "1.0.102",
            InstallKind::Nsis,
            vec![nsis("1.0.101"), nsis("1.0.102")],
        );
        assert_eq!(r.target.as_ref().map(|t| t.version.as_str()), Some("1.0.101"));
        assert!(r.delete.is_empty());
        assert_eq!(r.keep.len(), 2);
    }

    /// The archive never grows past two. Everything older than the target goes.
    #[test]
    fn only_the_seed_and_the_newest_older_installer_survive() {
        let r = retention(
            "1.0.104",
            InstallKind::Nsis,
            vec![nsis("1.0.100"), nsis("1.0.101"), nsis("1.0.103"), nsis("1.0.104")],
        );
        assert_eq!(r.target.as_ref().map(|t| t.version.as_str()), Some("1.0.103"));
        assert_eq!(
            r.keep,
            vec!["Spaceadom-1.0.104-installer.exe", "Spaceadom-1.0.103-installer.exe"]
        );
        assert_eq!(
            r.delete,
            vec!["Spaceadom-1.0.101-installer.exe", "Spaceadom-1.0.100-installer.exe"]
        );
    }

    /// A NEWER installer than the running version is the leftover of an update
    /// that did not take — a declined UAC, a failed install. It is neither the
    /// seed nor a way back, and the plugin re-downloads rather than reusing a
    /// local file, so keeping it buys nothing.
    #[test]
    fn an_installer_newer_than_the_running_version_is_pruned() {
        let r = retention(
            "1.0.101",
            InstallKind::Nsis,
            vec![nsis("1.0.101"), nsis("1.0.102")],
        );
        assert_eq!(r.delete, vec!["Spaceadom-1.0.102-installer.exe"]);
        assert_eq!(r.target, None);
    }

    /// PROBLEM 129/244 in the archive: both copies share `%APPDATA%`, so the
    /// per-machine copy's `.msi` can be sitting there while the per-user NSIS
    /// copy prunes. It must never be offered as a target — and never deleted
    /// either, because it is the OTHER copy's only way back.
    #[test]
    fn an_installer_of_the_other_kind_is_never_a_target_and_never_deleted() {
        let msi = ArchiveEntry {
            version: "1.0.101".into(),
            file: archive_name("1.0.101", InstallKind::Msi).unwrap(),
            kind: InstallKind::Msi,
        };
        let older_msi = ArchiveEntry {
            version: "1.0.99".into(),
            file: archive_name("1.0.99", InstallKind::Msi).unwrap(),
            kind: InstallKind::Msi,
        };
        let r = retention(
            "1.0.102",
            InstallKind::Nsis,
            vec![msi.clone(), older_msi.clone(), nsis("1.0.102")],
        );
        assert_eq!(r.target, None, "an .msi may never be run over an NSIS install");
        assert!(r.delete.is_empty(), "the other copy's installers are not ours to delete");
        assert!(r.keep.contains(&msi.file) && r.keep.contains(&older_msi.file));
    }

    #[test]
    fn retention_does_not_depend_on_the_order_the_directory_was_read_in() {
        let a = retention("1.0.104", InstallKind::Nsis, vec![nsis("1.0.100"), nsis("1.0.104"), nsis("1.0.103")]);
        let b = retention("1.0.104", InstallKind::Nsis, vec![nsis("1.0.103"), nsis("1.0.100"), nsis("1.0.104")]);
        assert_eq!(a, b);
    }

    /// The hold is what stops a rollback being undone by the next daily tick.
    /// REVIEW FIXES 2026-09-05 (MEDIUM). The case the reviewer named:
    /// `(false, NSIS_DIR, NSIS_DIR)` — no `uninstall.exe`, and an MSI product
    /// registered for a folder inside the user profile which IS this exe's
    /// folder. That used to answer `Msi`.
    #[test]
    fn an_msi_registration_inside_the_user_profile_is_never_an_msi_install() {
        assert_eq!(
            classify_install(false, Some(NSIS_DIR), NSIS_DIR),
            InstallKind::Unknown,
            "a per-machine .msi does not install into %LOCALAPPDATA%. An MSI registration \
             pointing there is PROBLEM 244's shape, and answering Msi is what aims the \
             updater — and eventually msiexec — at the per-user folder."
        );
        // With uninstall.exe present the same folder is still NSIS, unchanged:
        // that arm is decided before this one and is the whole reason
        // uninstall.exe wins over an MSI registration.
        assert_eq!(classify_install(true, Some(NSIS_DIR), NSIS_DIR), InstallKind::Nsis);
        // And a genuine per-machine install is untouched.
        assert_eq!(classify_install(false, Some(PF_DIR), PF_DIR), InstallKind::Msi);
    }

    #[test]
    fn per_user_locations_are_recognised_by_shape() {
        assert!(is_per_user_location(r"C:\Users\beamu\AppData\Local\Spaceadom"));
        assert!(is_per_user_location(r"c:/users/beamu/appdata/local/spaceadom"));
        assert!(is_per_user_location(r"D:\Data\AppData\Roaming\Spaceadom"));
        assert!(is_per_user_location("\"C:\\Users\\beamu\\AppData\\Local\\Spaceadom\""));
        assert!(!is_per_user_location(r"C:\Program Files\Spaceadom"));
        assert!(!is_per_user_location(r"C:\ProgramData\Spaceadom"));
        // The word has to be a PATH SEGMENT, not a substring of a name.
        assert!(!is_per_user_location(r"C:\Program Files\MyUsersApp"));
    }

    // -- REVIEW FIXES 2026-09-05 (H1) — the integrity seams -----------------

    /// A real minisign key pair's public half and one signature it made, taken
    /// from `minisign-verify 0.2.5`'s own test vector. Using someone else's
    /// published vector rather than our own key is deliberate: this asserts the
    /// ALGORITHM wiring (base64 layer, `allow_legacy`, key-id matching), and it
    /// keeps a real Spaceadom release signature out of the test suite where it
    /// would rot the day the key is rotated.
    const VECTOR_PUBKEY: &str = "untrusted comment: minisign public key E7620F1842B4E81F\n\
                                 RWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3";
    const VECTOR_SIG: &str = "untrusted comment: signature from minisign secret key\n\
        RWQf6LRCGA9i59SLOFxz6NxvASXDJeRtuZykwQepbDEGt87ig1BNpWaVWuNrm73YiIiJbq71Wi+dP9eKL8OC351vwIasSSbXxwA=\n\
        trusted comment: timestamp:1555779966\tfile:test\n\
        QtKMXWyYcwdpZAlPF7tE2ENJkRd1ujvKjlj1m9RtHTBnZPa5WKU5uWRs5GoP5M/VqE81QFuMKI5k/SfNQUaOAA==";

    fn b64(s: &str) -> String {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.encode(s)
    }

    #[test]
    fn a_correctly_signed_file_verifies_and_one_changed_byte_does_not() {
        let key = b64(VECTOR_PUBKEY);
        let sig = b64(VECTOR_SIG);
        verify_minisign(b"test", &sig, &key).expect("the signed bytes must verify");
        // The planted file. One byte different is the whole attack: the
        // archived installer is swapped for something else and run elevated.
        let why = verify_minisign(b"Test", &sig, &key)
            .expect_err("altered bytes must be REFUSED, not merely logged");
        assert!(
            why.contains("does not match"),
            "the refusal has to say what failed, not just fail: {why}"
        );
    }

    #[test]
    fn a_garbled_signature_or_key_is_refused_and_never_panics() {
        let key = b64(VECTOR_PUBKEY);
        let sig = b64(VECTOR_SIG);
        // Not base64 at all.
        assert!(verify_minisign(b"test", "!!!!not base64!!!!", &key).is_err());
        assert!(verify_minisign(b"test", &sig, "!!!!not base64!!!!").is_err());
        // Valid base64 of something that is not a minisign document.
        assert!(verify_minisign(b"test", &b64("hello"), &key).is_err());
        assert!(verify_minisign(b"test", &sig, &b64("hello")).is_err());
        // Empty, which is what reading a truncated .sig file gives.
        assert!(verify_minisign(b"test", "", &key).is_err());
    }

    #[test]
    fn the_shipped_public_key_is_a_decodable_minisign_key() {
        // Catches the failure mode that would otherwise disable rollback
        // silently: a pubkey in tauri.conf.json that is base64 of the wrong
        // thing verifies NOTHING, and every archived installer would be
        // refused for a reason that has nothing to do with the installer.
        const SHIPPED: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IENCNjgxQzA1NTlFMDQ4OTUKUldTVlNPQlpCUnhveTAzazU1enJoTktSWEdtV2NSYVNCU3U5N08yZGNJTVJkWUJWNTBqYnYrVVEK";
        let why = verify_minisign(b"anything", &b64(VECTOR_SIG), SHIPPED)
            .expect_err("a signature from a DIFFERENT key must not verify");
        assert!(
            !why.contains("public key"),
            "the shipped key itself must decode cleanly — the failure has to be about the \
             signature, not the key: {why}"
        );
    }

    #[test]
    fn the_signature_sits_beside_the_installer_and_is_never_an_archive_entry() {
        let p = std::path::Path::new(r"C:\x\Spaceadom-1.0.99-installer.exe");
        assert_eq!(
            sig_path_for(p),
            std::path::PathBuf::from(r"C:\x\Spaceadom-1.0.99-installer.exe.sig")
        );
        // The predicate that decides what this feature may delete must refuse
        // the signature on its own account, so a `.sig` can only ever be
        // removed as the companion of the installer it belongs to.
        assert_eq!(parse_archive_name("Spaceadom-1.0.99-installer.exe.sig"), None);
        assert_eq!(parse_archive_name("Spaceadom-1.0.99-installer.msi.sig"), None);
        // And the installers themselves still parse.
        assert!(parse_archive_name("Spaceadom-1.0.99-installer.exe").is_some());
    }

    #[test]
    fn sha256_is_the_real_thing_and_one_byte_changes_it() {
        // NIST's canonical vector, so a wrong hasher cannot pass.
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_ne!(sha256_hex(b"msi bytes"), sha256_hex(b"msi bytez"));
        assert_eq!(sha256_hex(b"abc").len(), 64, "lowercase hex, fixed width");
    }

    #[test]
    fn each_update_stages_into_its_own_unpredictable_temp_directory() {
        let a = temp_dir_suffix();
        let b = temp_dir_suffix();
        assert_eq!(a.len(), 16);
        assert_ne!(
            a, b,
            "two updates must never share a staging directory — a fixed %TEMP% name is what \
             let a planted file be swapped in between the write and the elevated launch"
        );
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn a_rollback_hold_blocks_the_daily_check_until_it_expires() {
        let hold = UpdateHold {
            until_ms: 1_000_000,
            pinned_version: "1.0.101".into(),
            reason: "rolled back".into(),
        };
        assert!(hold_blocks_daily_check(999_999, Some(&hold)));
        assert!(!hold_blocks_daily_check(1_000_000, Some(&hold)), "expiry is exclusive");
        assert!(!hold_blocks_daily_check(1_000_001, Some(&hold)));
        assert!(!hold_blocks_daily_check(0, None));
    }

    /// A hold written by a future build with extra fields must still parse, and
    /// a corrupt one must read as NO hold — a file that cannot be understood
    /// may not be allowed to freeze updates forever.
    #[test]
    fn a_hold_round_trips_and_a_broken_one_is_not_a_hold() {
        let hold = UpdateHold {
            until_ms: 42,
            pinned_version: "1.0.101".into(),
            reason: "r".into(),
        };
        let text = serde_json::to_string(&hold).unwrap();
        assert_eq!(serde_json::from_str::<UpdateHold>(&text).unwrap(), hold);
        assert!(serde_json::from_str::<UpdateHold>("{ not json").is_err());
        assert!(serde_json::from_str::<UpdateHold>("{}").is_err());
    }

    /// Two reasons to refuse a manual check, and they are NOT the same
    /// sentence: "still running" must never tell someone watching a progress
    /// bar to wait 30 seconds.
    #[test]
    fn the_manual_check_rate_limit_separates_running_from_just_ran() {
        let gap = Duration::from_secs(30);
        assert_eq!(manual_gate(false, None, gap), None, "the first check always runs");
        assert_eq!(manual_gate(false, Some(Duration::from_secs(30)), gap), None);
        assert_eq!(manual_gate(false, Some(Duration::from_secs(300)), gap), None);

        let busy = manual_gate(true, None, gap).expect("a check in flight must refuse");
        assert!(busy.contains("Already checking"), "{busy}");

        let soon = manual_gate(false, Some(Duration::from_secs(1)), gap).expect("29s ago must refuse");
        assert!(soon.contains("30 seconds"), "{soon}");
        assert!(!soon.contains("Already checking"), "{soon}");
        // In flight wins over the clock, so a long download never reports the
        // wrong reason.
        assert_eq!(manual_gate(true, Some(Duration::from_secs(1)), gap).as_deref(), Some("Already checking for updates…"));
    }

    /// Every state the UI can receive is a distinct, non-empty string. The
    /// contract batch 2 is written against.
    #[test]
    fn the_status_states_are_distinct_and_serialise_as_the_ui_expects() {
        let all = [
            STATE_CHECKING, STATE_UP_TO_DATE, STATE_DOWNLOADING,
            STATE_INSTALLING, STATE_ERROR, STATE_BUSY, STATE_NOT_ELIGIBLE,
        ];
        let mut seen = all.to_vec();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), all.len(), "two states share a wire value");
        assert!(all.iter().all(|s| !s.is_empty()));

        // `latest` and `progress` are omitted when absent, so a UI can test for
        // their presence rather than for a null.
        let s = UpdateStatus::plain(STATE_CHECKING, "1.0.100", "Checking for updates…");
        let j = serde_json::to_value(&s).unwrap();
        assert_eq!(j["state"], "checking");
        assert_eq!(j["current"], "1.0.100");
        assert_eq!(j["message"], "Checking for updates…");
        assert!(j.get("latest").is_none() && j.get("progress").is_none());

        let s = UpdateStatus {
            state: STATE_DOWNLOADING,
            current: "1.0.100".into(),
            latest: Some("1.0.101".into()),
            message: "Downloading version 1.0.101…".into(),
            progress: Some(45),
        };
        let j = serde_json::to_value(&s).unwrap();
        assert_eq!(j["latest"], "1.0.101");
        assert_eq!(j["progress"], 45);
    }

    /// Every status the user can be shown is a finished sentence, because the
    /// UI contract is "an unrecognised state falls back to `message`".
    #[test]
    fn every_msiexec_outcome_is_a_finished_sentence_and_never_a_bare_code() {
        for code in [1602, 1603, 1618, 1925, 4242] {
            let m = msiexec_reason(code);
            assert!(m.ends_with('.'), "not a sentence: {m}");
            assert!(m.starts_with(|c: char| c.is_ascii_uppercase()), "{m}");
            assert!(m.len() > 20, "{m}");
        }
        assert!(msiexec_reason(1602).contains("declined"));
        assert!(msiexec_reason(1925).contains("administrator"));
        assert!(msiexec_reason(4242).contains("4242"));
    }

    /// The rollback runs the archived installer with the SAME switches the
    /// updater uses for a new one — the whole point is that it is an ordinary
    /// install of an older package, not a special path.
    #[test]
    fn the_msi_rollback_uses_the_identical_command_line_as_an_msi_update() {
        let p = msi_parameters(r"C:\Users\beamu\AppData\Roaming\Spaceadom\rollback\Spaceadom-1.0.101-installer.msi");
        assert!(p.contains("/passive") && p.contains("REBOOT=ReallySuppress"));
        assert!(p.contains("AUTOLAUNCHAPP=True") && p.contains("LAUNCHAPPARGS=\"--autostart\""));
        assert!(p.starts_with("/i \""), "{p}");
    }

    /// 24 h, in the units the file actually stores.
    #[test]
    fn the_rollback_hold_is_twenty_four_hours() {
        assert_eq!(ROLLBACK_HOLD.as_secs(), 24 * 60 * 60);
        assert_eq!(MANUAL_MIN_GAP.as_secs(), 30);
        let now = now_ms();
        let hold = UpdateHold {
            until_ms: now + ROLLBACK_HOLD.as_millis() as u64,
            pinned_version: "1.0.101".into(),
            reason: String::new(),
        };
        assert!(hold_blocks_daily_check(now, Some(&hold)));
        assert!(hold_blocks_daily_check(now + 23 * 3_600_000, Some(&hold)));
        assert!(!hold_blocks_daily_check(now + 25 * 3_600_000, Some(&hold)));
    }
}
