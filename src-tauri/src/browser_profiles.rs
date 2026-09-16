/// browser_profiles.rs — detect Chromium browsers and their profiles, and
/// decide how a binding that pins one should be launched.
///
/// WHAT THIS IS FOR. Chromium browsers take `--profile-directory="Profile 1"`
/// on the command line and open straight into that profile. The mapping from
/// that internal folder name to the human one ("ARPON'S STUDIES") lives in
/// `<product folder>\User Data\Local State`, a JSON file with a
/// `profile.info_cache` object keyed by folder name. This module finds those
/// files, turns them into a pickable list, and owns every branch that decides
/// whether a key press uses that machinery or the untouched default-browser
/// path.
///
/// NO VENDOR LIST, BY REQUIREMENT. The owner asked for Arc, Helium "and
/// anything else" to be found automatically, so nothing here matches on a
/// browser's name. Detection is structural: find `Local State`, check it has
/// the Chromium profile shape, then try to resolve an executable for it.
///
/// ---------------------------------------------------------------------------
/// WHAT THE REAL MACHINE ACTUALLY LOOKS LIKE (measured 2026-08-26, not assumed)
/// ---------------------------------------------------------------------------
///
/// The obvious heuristic — "the exe is at `<product folder>\Application\*.exe`"
/// — is WRONG for most browsers, and measuring is what caught it:
///
/// ```text
///   Chrome   user data %LOCALAPPDATA%\Google\Chrome\User Data
///            exe       C:\Program Files\Google\Chrome\Application\chrome.exe
///            -> %LOCALAPPDATA%\Google\Chrome has NO Application folder at all
///
///   Edge     user data %LOCALAPPDATA%\Microsoft\Edge\User Data
///            exe       C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe
///            -> same: nothing under the LOCALAPPDATA product folder
///
///   Brave    user data %LOCALAPPDATA%\BraveSoftware\Brave-Browser\User Data
///            exe       C:\Program Files\BraveSoftware\Brave-Browser\Application\brave.exe
///            -> %LOCALAPPDATA%\BraveSoftware\Brave-Browser\Application EXISTS
///               but contains ONLY a stale versioned folder (147.1.89.132) and
///               NO exe. A naive scan finds the folder, finds nothing in it,
///               and would have to guess.
///
///   Samsung  user data %LOCALAPPDATA%\Samsung\Internet\User Data
///            exe       C:\Program Files\Samsung\Internet\Application\samsunginternet.exe
///            -> note the sibling samsunginternet_proxy.exe, which must not win
///
///   Arc      user data %LOCALAPPDATA%\Packages\TheBrowserCompany.Arc_ttt1ap7aakyb4
///                      \LocalCache\Local\Arc\User Data              (depth SIX)
///            exe       %LOCALAPPDATA%\Microsoft\WindowsApps\Arc.exe (MSIX alias,
///                      a 0-byte reparse point; no Application folder anywhere)
/// ```
///
/// So resolution has THREE sources, tried in order, and a browser that matches
/// none of them is dropped rather than guessed at — a browser missing from the
/// picker is a far smaller failure than a picker entry that launches the wrong
/// program.
///
/// THE OPERA LAYOUT (added 2026-08-26, owner's decision: "Add it, but log
/// clearly when a browser is skipped"). Opera does not use the
/// `<product>\User Data\Local State` nesting at all. **REASONED, NOT MEASURED:
/// neither Opera nor Vivaldi is installed on this machine, so unlike everything
/// in the table above, the following comes from Opera's public
/// documentation/forums (Opera 114+ layout) and could not be verified against a
/// live install:**
///
/// ```text
///   Opera     user data %APPDATA%\Opera Software\Opera Stable   (ROAMING, and
///             `Local State` + the `Default` profile folder sit DIRECTLY in it
///             — there is no `User Data` level)
///             exe       %LOCALAPPDATA%\Programs\Opera\opera.exe (per-user) or
///                       %PROGRAMFILES%\Opera\opera.exe          (per-machine)
///   Opera GX  user data %APPDATA%\Opera Software\Opera GX Stable
///             exe       %LOCALAPPDATA%\Programs\Opera GX\opera.exe
/// ```
///
/// Three consequences, each handled by extending an EXISTING source rather than
/// adding a fourth:
///   · `product_dir_for` — the product folder is the data dir ITSELF when the
///     `Local State` is not inside a `User Data` folder (the same shape most
///     embedded CEF apps use; Spotify, measured on this machine, is the live
///     example).
///   · `registered_browsers` no longer drops entries whose exe is not inside an
///     `Application` folder — Opera registers under `StartMenuInternet` but its
///     exe sits directly in `…\Programs\Opera\`. Internet Explorer, which that
///     filter used to exclude, is still excluded for a better reason: it has no
///     Chromium-shaped data dir, so it can never marry a candidate.
///   · `product_paths_match` tolerates Opera's channel suffix — the data folder
///     is "Opera Stable"/"Opera GX Stable" while the install folder is
///     "Opera"/"Opera GX".
///
/// VIVALDI NEEDS NOTHING. Its documented layout is the standard nesting
/// (`%LOCALAPPDATA%\Vivaldi\User Data` + `%LOCALAPPDATA%\Vivaldi\Application\
/// vivaldi.exe`), which source 1 resolves for the per-user install and the
/// registry + one-component leaf match already resolves for a per-machine
/// `C:\Program Files\Vivaldi` — the `product_paths_match` doc comment has used
/// exactly that path as its worked example since the day it was written.
///
/// THE `profile.info_cache` CHECK IS NOT SUFFICIENT ON ITS OWN. The brief that
/// commissioned this expected that shape to be self-validating. Measured, it is
/// not: on this machine 66 files named `Local State` exist under AppData, and
/// EVERY WebView2 host (`…\EBWebView\Local State` — about forty of them,
/// including this app's own) carries a `profile.info_cache` with a "Profile 1"
/// in it, as does Spotify. Requiring a resolvable browser EXECUTABLE is what
/// actually separates a browser from a WebView2 data folder, and it is why
/// resolution failure means "skip", not "fall back to something".
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// The shape handed to the frontend
// ---------------------------------------------------------------------------

/// One profile inside one browser.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserProfile {
    /// The Chromium INTERNAL folder name — what `--profile-directory=` wants.
    pub directory: String,
    /// The human name from `info_cache[dir].name`, falling back to `directory`.
    pub display_name: String,
    /// The signed-in account email, from `info_cache[dir].user_name`. `None`
    /// when the profile is not signed in — never rendered as an empty line.
    ///
    /// MEASURED 2026-08-31 against the owner's real `Local State` files
    /// (Chrome, Edge, Brave, Samsung Internet), not assumed:
    ///
    ///   · Chrome — every signed-in profile carries a real email in
    ///     `user_name`, one per profile (14 profiles, all populated; the
    ///     actual addresses are not reproduced here — real personal Gmail
    ///     accounts have no business sitting in a source comment).
    ///     `gaia_name` is ALSO populated on every one of them, but it
    ///     is the Google account's DISPLAY name ("Nur Arpon"), not an email —
    ///     using it as an email fallback would show a person's name where an
    ///     address belongs.
    ///   · Edge (`Profile 1`, not signed in) — `user_name` is `""`: the key is
    ///     PRESENT but EMPTY, not absent. So is `gaia_name`.
    ///   · Brave (`Default` and the owner's `"ARPON'S STUDIES"` profile,
    ///     neither signed in) — `user_name` is `""` again, but `gaia_name` is
    ///     not in the JSON AT ALL for Brave's shape — the key is missing
    ///     outright, not merely empty. A `gaia_name`-based fallback would have
    ///     hit `None` here for a different reason than "not signed in", which
    ///     is exactly the kind of two-causes-one-symptom trap this file's
    ///     header warns about elsewhere.
    ///   · Samsung Internet (`Default`, not signed in) — `user_name` is `""`,
    ///     `gaia_name` is `""`.
    ///
    /// Conclusion acted on below: `user_name` is the ONLY field that is ever
    /// actually shaped like an email, so it is the only one used. Both
    /// "present but blank" and "absent entirely" collapse to `None` here —
    /// the frontend must never be able to tell the two apart, because neither
    /// means anything different to a user who is not signed in.
    pub email: Option<String>,
    /// **The one label the user actually reads.** The LOCAL PART of `email`
    /// — everything before the `@` — falling back to `display_name` when the
    /// profile is not signed in.
    ///
    /// Owner's decision 2026-08-31, reversing the shipped-in-tree design that
    /// led with `display_name` and put the full address on a second line. His
    /// reasoning: on a machine with 14 Chrome profiles the display names are
    /// near-identical ("Person 1", "Person 3"), the addresses are what tell
    /// them apart, and a full address is too long for a HUD chip that caps at
    /// 118px. The local part is the shortest thing that is still unique.
    ///
    /// Computed HERE, in Rust, rather than in each of the four places that
    /// render a profile (picker headline, HUD ring chip, key-editor chip,
    /// toasts). One rule, one test, no drift — and the frontend renders a
    /// string it is handed instead of re-deriving one.
    ///
    /// The FULL address never appears in any of those four; it appears only in
    /// a hover tooltip, which the frontend builds from `email` itself.
    pub account_label: String,
}

/// The local part of an email address — everything before the `@`.
///
/// `None` when the input is not actually address-shaped, which is a real case
/// and not paranoia: `user_name` is a free-text field in someone else's JSON,
/// and a Chromium fork is free to put anything there. The rule is deliberately
/// strict — there must be exactly one usable `@`, a non-empty local part and a
/// non-empty domain — because the fallback (`display_name`) is always a sane
/// thing to show, so a doubtful value is better refused than half-rendered.
///
/// Generalise: when a derived label has a good fallback, validate the source
/// hard. It is the cases with NO fallback that have to accept whatever they get.
pub fn email_local_part(email: &str) -> Option<String> {
    let e = email.trim();
    let (local, domain) = e.split_once('@')?;
    // A second `@` means this is not an address in any form we can reason
    // about. Refuse rather than guess which one splits it.
    if domain.contains('@') {
        return None;
    }
    let local = local.trim();
    if local.is_empty() || domain.trim().is_empty() {
        return None;
    }
    Some(local.to_string())
}

/// The label for one profile: the email's local part when signed in, the
/// browser's own display name when not.
///
/// Pure and separate from `BrowserProfile` so the rule can be tested without
/// building one, the same way `hud_label` is.
pub fn account_label(display_name: &str, email: Option<&str>) -> String {
    email
        .and_then(email_local_part)
        .filter(|l| !l.is_empty())
        .unwrap_or_else(|| display_name.to_string())
}

/// One detected Chromium browser.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedBrowser {
    /// Human name. Windows' own name for the browser when it is a registered
    /// default-browser candidate ("Google Chrome", "Brave"), otherwise the
    /// product folder humanised.
    pub browser_name: String,
    /// Absolute path to the launcher exe.
    pub browser_exe: String,
    /// The `User Data` directory its profiles live in. Kept so the frontend can
    /// show it as a tooltip and so a caller has it without re-deriving.
    pub user_data_dir: String,
    /// Base64 PNG, same extractor and same cache as `list_start_menu_apps`.
    pub icon_base64: Option<String>,
    /// Profiles whose folder still exists, `Default` first then alphabetical.
    pub profiles: Vec<BrowserProfile>,
}

// ---------------------------------------------------------------------------
// PURE DECISION LOGIC — the part that is actually tested
// ---------------------------------------------------------------------------

/// **The guard the whole hard requirement rests on.**
///
/// The owner said it twice, in these words: *"MAKE SURE THE DEFAULT BROWSER
/// LAUNCHES FROM URL IF NOT EXPLICITLY SET TO SPECIFIC."* So there is exactly
/// one question — did the user explicitly pin a browser? — and it is asked
/// here, in one place, by `smart_cascade` before it chooses a path.
///
/// Trivial on purpose. It is tested anyway because it is the ONLY thing
/// standing between every existing binding and a behaviour change, and a
/// regression in it would be invisible until someone's links started opening in
/// a browser they never chose.
///
/// An empty or whitespace-only string counts as NOT set. That is not
/// hair-splitting: a frontend that writes `""` instead of `null` is an ordinary
/// bug, and the correct response to it is the default browser, not a
/// `ShellExecute` on an empty path.
pub fn should_use_specific_browser(binding: &crate::config::KeyBinding) -> bool {
    binding
        .browser_exe
        .as_deref()
        .is_some_and(|s| !s.trim().is_empty())
}

/// Which route a `web_url` binding takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserRoute {
    /// `run_browser(url)` — today's behaviour, byte for byte.
    Default,
    /// `shell_launch(exe, params)` into a specific browser.
    Specific { exe: String },
    /// A browser WAS pinned, and its exe is gone. Falls back to `Default`, but
    /// as a distinct variant so the caller can WARN with the missing path
    /// instead of silently behaving like a binding that was never pinned.
    PinnedBrowserMissing { exe: String },
}

/// Choose the route for a binding, given a way to ask whether a path exists.
///
/// The existence check is injected so the decision can be tested without a real
/// browser on disk, and so the test can prove the polarity of the fallback
/// rather than trusting it.
///
/// THE UNINSTALLED-BROWSER FALLBACK is the same shape as the stale-profile one
/// the owner approved, one level up: a key whose pinned browser has been
/// uninstalled opens its URL in the default browser rather than doing nothing
/// at all. A dead key is the worse failure — the user cannot tell it from the
/// app being broken.
pub fn route_for(
    binding: &crate::config::KeyBinding,
    exists: &dyn Fn(&Path) -> bool,
) -> BrowserRoute {
    if !should_use_specific_browser(binding) {
        return BrowserRoute::Default;
    }
    // `should_use_specific_browser` just proved this is Some and non-blank.
    let exe = binding.browser_exe.as_deref().unwrap_or("").trim().to_string();
    if exists(Path::new(&exe)) {
        BrowserRoute::Specific { exe }
    } else {
        BrowserRoute::PinnedBrowserMissing { exe }
    }
}

/// What to do about a requested `--profile-directory=`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileArg {
    /// No profile was requested — launch the browser normally.
    None,
    /// Pass `--profile-directory="<dir>"`.
    Use(String),
    /// A profile WAS requested and its folder is gone (the user deleted it in
    /// the browser). Launch WITHOUT the parameter — the browser then opens its
    /// own default/last-used profile — and WARN.
    DroppedStale {
        dir: String,
        /// The `User Data` directory that was searched, for the log line.
        searched: PathBuf,
    },
}

/// Where a browser exe's `User Data` directory is, or `None` if it cannot be
/// located confidently.
///
/// Cannot be read off the exe path alone — Chrome's exe is in Program Files
/// while its profiles are in LOCALAPPDATA — so three candidates are tried, in
/// the order that matches how the real installs on this machine are laid out:
///
///   1. `<exe>\..\..\User Data` — a per-user install that keeps both together.
///   2. `%LOCALAPPDATA%\<vendor>\<product>\User Data`, taking the last TWO
///      components of the exe's product folder. This is the one that resolves
///      Chrome (`Google\Chrome`), Edge (`Microsoft\Edge`), Brave
///      (`BraveSoftware\Brave-Browser`) and Samsung (`Samsung\Internet`).
///   3. `%LOCALAPPDATA%\<product>\User Data` — the one-component form, for a
///      vendor that does not nest.
///
/// Returning `None` is a real answer, not a failure: Arc's MSIX alias has no
/// derivable product folder, and the caller's rule for `None` is to KEEP the
/// profile argument (see `profile_arg_for`).
pub fn user_data_dir_for(
    browser_exe: &Path,
    local_app_data: Option<&Path>,
    exists: &dyn Fn(&Path) -> bool,
) -> Option<PathBuf> {
    // `<product>\Application\browser.exe` -> `<product>`
    let product = browser_exe.parent().and_then(|p| p.parent());

    if let Some(prod) = product {
        let direct = prod.join("User Data");
        if exists(&direct) {
            return Some(direct);
        }
    }

    let lad = local_app_data?;
    let prod = product?;
    let comps: Vec<_> = prod
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();

    // Two-component form first: it is strictly more specific, so it cannot be
    // shadowed by a coincidental one-component match.
    for take in [2usize, 1] {
        if comps.len() < take {
            continue;
        }
        let mut cand = lad.to_path_buf();
        for c in &comps[comps.len() - take..] {
            cand.push(c);
        }
        cand.push("User Data");
        if exists(&cand) {
            return Some(cand);
        }
    }
    None
}

/// Decide whether a pinned profile folder may still be used.
///
/// THE POLARITY HERE IS THE WHOLE POINT and it is asymmetric on purpose:
///
///   · profile requested, `User Data` found, folder present  -> use it
///   · profile requested, `User Data` found, folder ABSENT    -> drop it + WARN
///   · profile requested, `User Data` NOT found               -> **use it**
///
/// That last line is the one worth arguing about. Dropping the argument because
/// we could not find the folder to check would break a launch that works fine —
/// Arc's MSIX alias has no derivable `User Data` path, and any browser laid out
/// in a way this code has not met would look identical. "Could not verify" must
/// never be treated as "verified absent"; only a positively located directory
/// that positively lacks the folder is evidence of anything.
pub fn profile_arg_for(
    browser_exe: &str,
    profile_dir: Option<&str>,
    local_app_data: Option<&Path>,
    exists: &dyn Fn(&Path) -> bool,
) -> ProfileArg {
    let dir = match profile_dir.map(str::trim) {
        Some(d) if !d.is_empty() => d.to_string(),
        _ => return ProfileArg::None,
    };

    let Some(user_data) = user_data_dir_for(Path::new(browser_exe), local_app_data, exists) else {
        // Unverifiable, so unchanged. See the doc comment above.
        return ProfileArg::Use(dir);
    };

    if exists(&user_data.join(&dir)) {
        ProfileArg::Use(dir)
    } else {
        ProfileArg::DroppedStale { dir, searched: user_data }
    }
}

/// Build the `lpParameters` string for `shell_launch`.
///
/// Chromium wants switches before the positional URL. Both parts are quoted: a
/// profile folder name can contain spaces ("Profile 1" always does), and a URL
/// can too once someone pastes one with an unencoded space in it.
pub fn build_launch_params(profile_dir: Option<&str>, url: Option<&str>) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    if let Some(d) = profile_dir.map(str::trim).filter(|d| !d.is_empty()) {
        parts.push(format!("--profile-directory=\"{d}\""));
    }
    if let Some(u) = url.map(str::trim).filter(|u| !u.is_empty()) {
        parts.push(format!("\"{u}\""));
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" "))
    }
}

// ---------------------------------------------------------------------------
// WINDOW -> PROFILE: the MATCHING side (2026-08-27)
// ---------------------------------------------------------------------------
//
// THE BUG THIS EXISTS FOR. Space+B was bound to one Brave profile and Space+N
// to a DIFFERENT Brave profile. Pressing Space+N MINIMISED Space+B's window,
// and Space+N's own profile never launched at all. The owner's live log:
//
//   23:44:07.905  Target: ...\brave.exe | HWND: HWND(0x40966) | Action: Restore (Enum)
//   23:44:10.136  Target: ...\brave.exe | HWND: HWND(0x40966) | Action: Minimize
//
// Identical target string, IDENTICAL HWND. The cascade's whole notion of
// "which window belongs to this binding" was the EXE FILE STEM, and two Brave
// bindings are the same string.
//
// WHAT DOES NOT WORK, measured on this machine 2026-08-26/27. Do not retry
// any of these; they were tested and are dead ends:
//
//   * COMMAND LINES. Brave has exactly ONE browser process (PID 30744) whose
//     command line contains NO --profile-directory at all, yet it owns a
//     Profile 1 window. Chrome the same, one process (PID 46752). One browser
//     process per user-data-dir hosts EVERY profile, and the Chromium
//     singleton lockfile is at `...\User Data\lockfile`, not per profile.
//     Win32_Process matching cannot distinguish profiles. At all.
//   * WINDOW TITLES. Measured on two non-default profiles: "Toxic: A Fairy
//     Tale... - Brave" and "Best VPN Online... - Google Chrome". Plain
//     `<tab title> - <Browser>`, no profile marker anywhere.
//   * CLASS NAMES. `Chrome_WidgetWin_1` for both. No suffix.
//
// WHAT DOES WORK. Chromium stamps the profile on EVERY browser window
// individually, in that window's own property store:
//
//   HWND 0x40966  AUMID    Brave.UserData.Profile1
//                 Relaunch "...\brave.exe" --profile-directory="Profile 1"
//   HWND 0x170978 AUMID    Chrome.UserData.Profile6
//                 Relaunch "...\chrome.exe" --profile-directory="Profile 6"
//
// AND THE DEFAULT-PROFILE SHAPE, which was the open question this whole design
// had to be safe without. Measured 2026-08-27 by `live_profile_probe` in
// `smart_cascade.rs` - i.e. by THIS code, natively, not through PowerShell -
// on a live Edge window:
//
//   msedge        AUMID    MSEdge
//                 Relaunch "...\msedge.exe" --profile-directory=Default
//
// Two things fall out of it, and neither could have been guessed:
//   * the value is UNQUOTED. Chromium quotes `"Profile 6"` because of the
//     space and leaves `Default` bare. A parser that only understood the
//     quoted form would return nothing here, and every Edge binding would
//     relaunch instead of toggling, forever, with no error.
//   * the AUMID is BARE - `MSEdge`, with no `.UserData.<profile>` segment at
//     all. So the AUMID reader must be allowed to find nothing without that
//     meaning the window is unidentified; the relaunch command is what carries
//     the answer here.
//
// Measured cost 0.026 ms per read (200 reads in 5.2 ms through PowerShell COM
// interop, so that is an upper bound - the native path is faster).
//
// THE TRAP THAT WOULD SILENTLY MATCH NOTHING: the two properties do not agree
// on spelling. RelaunchCommand carries the literal folder name WITH its space,
// `Profile 1`. The AUMID STRIPS the space, `Profile1`. Comparing one against
// the other verbatim finds nothing, forever, with no error and no log line.
// Every comparison in this app goes through `same_profile_dir`, which is the
// only reason that function exists and why it has a test of its own.
//
// GENERALISE THIS - deliberately NOT done in this pass. Nothing about the
// shape of this bug is browser-specific: two .lnk shortcuts with different
// arguments, two Windows Terminal profiles, two VS Code workspaces and two
// Store apps sharing a package family all collide the same way, because they
// all reduce to one exe stem. The general fix is these same three pieces - a
// binding-identity cache key, per-window evidence, and "no proof means do not
// touch it" - with a different evidence reader per family. Widening the scope
// inside the change that fixes the browser case is what would put the cascade
// at risk, which is what CORE_AIM.md protects.

/// The `--profile-directory=` value out of a window's `RelaunchCommand`
/// (`PKEY_AppUserModel_RelaunchCommand`, fmtid 9F4C2855-..., pid 2).
///
/// PRIMARY evidence. This is the one property that carries the profile's
/// LITERAL folder name, space and all, exactly as `--profile-directory=` wants
/// it and exactly as `build_launch_params` writes it. The parser and the
/// builder are inverses and live next to each other on purpose - if one is
/// ever changed, the diff should make it obvious that the other needs the same
/// change.
///
/// `None` means "this command line NAMES no profile", which is emphatically
/// not the same as "this window has no profile". Callers must treat `None` as
/// *unproven*, never as *proven absent*.
///
/// THE DEFAULT PROFILE, measured 2026-08-27 on a live Edge window: it does
/// carry the switch, and it carries it UNQUOTED -
/// `--profile-directory=Default`. That is why the unquoted branch below is not
/// defensive padding: without it every single-profile browser window would come
/// back unidentified and every such binding would relaunch instead of toggling.
/// A window whose relaunch command omits the switch entirely has still never
/// been observed; that case stays deliberately unhandled and lands on
/// `WindowProfile::NoProfileNamed`, which proves nothing on its own.
pub fn profile_dir_from_relaunch_command(cmd: &str) -> Option<String> {
    const FLAG: &str = "--profile-directory=";
    // ASCII-lowercase, never `to_lowercase()`: a browser installed under a
    // path containing non-ASCII characters can change BYTE LENGTH under a
    // Unicode lowercase, and every index taken from the lowered copy would
    // then point at the wrong place in the original.
    let hay = cmd.to_ascii_lowercase();
    let at = hay.find(FLAG)? + FLAG.len();
    let rest = &cmd[at..];
    let value = if let Some(inner) = rest.strip_prefix('"') {
        // Measured shape: --profile-directory="Profile 1"
        inner.split('"').next().unwrap_or("")
    } else {
        // Unquoted. Chromium quotes anything containing a space, so an
        // unquoted value is a single whitespace-delimited token by
        // construction.
        rest.split_whitespace().next().unwrap_or("")
    };
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

/// The profile token out of a Chromium window's AUMID
/// (`PKEY_AppUserModel_ID`, same fmtid, pid 5).
/// `Brave.UserData.Profile1` -> `Profile1`.
///
/// CORROBORATION, not primary - and note the missing space. Anything compared
/// against this MUST go through `same_profile_dir`; see the trap paragraph
/// above.
///
/// Only the measured `<browser>.UserData.<profile>` shape is parsed, and
/// `None` is an ordinary answer rather than a failure: a live Edge window
/// measured 2026-08-27 reports the BARE AUMID `MSEdge`, with no profile
/// segment at all, and its RelaunchCommand is what identifies it. A browser
/// launched with a custom `--user-data-dir` produces a shape that was not
/// measured and also returns `None` here - which lands the caller in the safe
/// direction (no proof, so no minimise) rather than in a guess.
pub fn profile_token_from_aumid(aumid: &str) -> Option<String> {
    const MID: &str = ".userdata.";
    let hay = aumid.to_ascii_lowercase();
    let at = hay.find(MID)? + MID.len();
    let token = aumid[at..].trim();
    if token.is_empty() {
        None
    } else {
        Some(token.to_string())
    }
}

/// Do two profile names refer to the same profile?
///
/// THE WHOLE POINT: `"Profile 1"` (RelaunchCommand) and `"Profile1"` (AUMID)
/// are the same profile written two ways by the same browser about the same
/// window. Whitespace is removed from both sides and both are lowercased, so
/// the comparison cannot depend on which property happened to be readable.
///
/// The cost of stripping whitespace symmetrically is that two folders named
/// `Profile 1` and `Profile1` would be treated as one. Chromium generates
/// profile folders itself (`Default`, `Profile 2`, `Profile 3`, ...) and gives
/// the user no way to rename the FOLDER, so that pair cannot occur in
/// practice - and a false match between two folders that do not exist is a
/// better trade than never matching the ones that do.
///
/// An empty name never matches anything, including another empty one: "we read
/// nothing" must not compare equal to "we read nothing".
pub fn same_profile_dir(a: &str, b: &str) -> bool {
    let a = normalise_profile_dir(a);
    !a.is_empty() && a == normalise_profile_dir(b)
}

/// One profile name, reduced to the form two of them are compared in.
///
/// Exposed because the cascade's HWND cache key is built from it too: the key
/// and the comparison must agree about what counts as the same profile, and
/// deriving that twice is how they would drift.
pub fn normalise_profile_dir(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// The `(browser exe stem, profile folder)` pairs that bindings have PINNED.
///
/// This is the input to the owner's rule for UNPINNED browser bindings
/// (decided 2026-08-27, not to be re-opened): *an unpinned browser binding
/// matches any window of that browser EXCEPT one whose profile is claimed by
/// another binding's pin in the active profile.* So Space+B, which pins
/// nothing, takes whichever Brave window is not Space+N's, rather than
/// stealing it.
///
/// A binding never needs to be excluded from its own claim list. A PINNED
/// binding does not consult claims at all - it demands positive proof of its
/// own profile - and an UNPINNED binding contributes no claim by definition.
/// That is worth stating because "exclude self" is the obvious-looking
/// requirement here, and implementing it would mean threading a key identity
/// through the cascade for no behavioural difference whatsoever.
pub fn profile_claims<'a, I>(bindings: I) -> Vec<(String, String)>
where
    I: IntoIterator<Item = &'a crate::config::KeyBinding>,
{
    let mut out: Vec<(String, String)> = Vec::new();
    for b in bindings {
        let Some(dir) = b
            .browser_profile_dir
            .as_deref()
            .map(str::trim)
            .filter(|d| !d.is_empty())
        else {
            continue;
        };
        // WHICH browser the pin is about. A URL binding names it in
        // `browser_exe`; an app binding's `app` IS the browser exe (that is
        // what "just open Brave's Studies profile" stores). Same precedence
        // the launch leg uses, so a claim can never disagree with the launch
        // it describes.
        let Some(exe) = b
            .browser_exe
            .as_deref()
            .map(str::trim)
            .filter(|e| !e.is_empty())
            .or_else(|| {
                b.app
                    .as_deref()
                    .map(str::trim)
                    .filter(|a| !a.is_empty())
            })
        else {
            continue;
        };
        let stem = Path::new(exe)
            .file_stem()
            .map(|s| s.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if stem.is_empty() {
            continue;
        }
        if !out
            .iter()
            .any(|(s, d)| s == &stem && same_profile_dir(d, dir))
        {
            out.push((stem, dir.to_string()));
        }
    }
    out
}

/// Every pin the user can reach with ONE key press right now: the ACTIVE
/// profile's bindings, plus the special keys (F1-F12, Enter, Tab, arrows),
/// which are global rather than per-profile.
///
/// Including the special keys widens the claim set slightly beyond the literal
/// words of the owner's rule, and that direction is deliberate: every extra
/// claim can only ever make an unpinned binding DECLINE a window and launch
/// instead, which is the safe half of this feature. A missed claim is what
/// minimises the wrong window. They also belong here on the merits — the
/// special keys are one press away right now, and they are machine-global
/// rather than another profile's configuration.
///
/// THE ONE BOUNDARY, stated so nobody has to rediscover it: the FOUNDERS
/// profile's bindings do NOT claim. A key left unassigned in the active profile
/// falls through to its Founders binding (see `handle_alpha`), so a Founders
/// pin is technically reachable, and an unpinned key could still take its
/// window. That case is narrow — it needs an unassigned key whose Founders
/// binding pins a profile of a browser another key uses unpinned — and the
/// owner's rule says "the active profile". Widening a decision he made is not
/// this change's to take; if it ever bites, one more `.chain()` here is the
/// whole fix.
///
/// COST, because this runs on the Space-hold latency path: the config is an
/// in-memory `Arc<RwLock<AppConfig>>`, and the engine already holds the read
/// guard open to look the binding up. This walk happens inside that same
/// guard - no second lock, no file I/O, no cache to invalidate - and for the
/// overwhelming majority of users, who pin nothing, it visits ~26 bindings,
/// allocates nothing and returns an empty Vec. A cached claim set was
/// considered and rejected: a stale cache here means acting on a claim that no
/// longer exists, which is the same class of bug as the one being fixed.
pub fn active_profile_claims(cfg: &crate::config::AppConfig) -> Vec<(String, String)> {
    let active = cfg.profiles.iter().find(|p| p.name == cfg.active_profile);
    profile_claims(
        active
            .into_iter()
            .flat_map(|p| p.bindings.values())
            .chain(cfg.special_keys.values()),
    )
}

/// The Guide HUD's label for a binding: `"Brave — Studies"` when a profile is
/// pinned, and EXACTLY the existing label when one is not.
///
/// Kept here rather than inline in `engine/mod.rs` so the "unchanged when no
/// profile is set" half is testable without standing up an engine.
pub fn hud_label(base: &str, profile_name: Option<&str>) -> String {
    match profile_name.map(str::trim).filter(|p| !p.is_empty()) {
        // An em dash, matching the app's own typography. The HUD chip already
        // truncates with an ellipsis at 118px (`.st-chip span` in
        // overlay-earthy.css) and the ring measures REAL rendered widths, so a
        // long pair cannot break the layout — it just ellipses like any other
        // long label does today.
        Some(p) if !base.eq_ignore_ascii_case(p) => format!("{base} — {p}"),
        // A profile whose name IS the browser's name would render as
        // "Brave — Brave". Say it once.
        _ => base.to_string(),
    }
}

// ---------------------------------------------------------------------------
// The two fallback reasons, as sentences (2026-08-26)
// ---------------------------------------------------------------------------
//
// The owner's design handoff, §5 "Failure states", ends: *"Two different
// failures get two different sentences. 'Something went wrong' leaves the user
// guessing which half to fix."* Both are already MODELLED here —
// `BrowserRoute::PinnedBrowserMissing` and `ProfileArg::DroppedStale` — and
// both were already logged. What neither reached was the user, who does not
// read `debug.log`: they pressed a key, something opened, and nothing said it
// was not what they pinned.
//
// The wording is the handoff's, verbatim, plus the leading glyph every toast
// in this app carries (`toast.ts` peels the first glyph off and renders it as
// the icon disc, then shows the rest — so "⚠️ Brave is gone — opened in Edge"
// displays exactly the handoff's sentence). ⚠️ is what `cascade_toast` already
// uses for "it worked, but not the way you asked".

/// Human name for a browser we can only identify by its executable path.
///
/// There is no better source, and that is worth stating because it looks like
/// there should be. A `PinnedBrowserMissing` browser is BY DEFINITION
/// uninstalled: `scan_browsers()` cannot see it (every resolution source it has
/// requires a live exe), Windows' `StartMenuInternet` name for it went with the
/// uninstall, and `KeyBinding` stores no browser name — its three browser
/// fields are the exe path, the profile FOLDER and the profile NAME. The stem
/// of the stored path is the last evidence on the machine.
///
/// The table names ONLY the stems where title-casing gives a WRONG answer.
/// Everything else — brave, chrome, firefox, vivaldi, arc, zen — title-cases
/// correctly, and a table that listed them too would be a list to forget to
/// update rather than a fix for anything.
pub fn display_name_for_exe(exe: &str) -> String {
    let stem = Path::new(exe.trim())
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    display_name_for_stem(&stem)
}

/// The same, for a bare process stem — what `browser_stem()` resolves for the
/// OS default browser (lowercased, e.g. "msedge").
pub fn display_name_for_stem(stem: &str) -> String {
    let key = stem.trim().to_lowercase();
    match key.as_str() {
        // Windows' own binary name for Edge. "Msedge" is not a word anyone
        // would recognise as their browser.
        "msedge" => return "Edge".into(),
        "iexplore" => return "Internet Explorer".into(),
        "samsunginternet" => return "Samsung Internet".into(),
        "opera_gx" | "operagx" => return "Opera GX".into(),
        "librewolf" => return "LibreWolf".into(),
        _ => {}
    }
    if key.is_empty() {
        // A binding whose stored path has no file name at all. Still a
        // sentence, still true, and it names the OTHER half of the failure.
        return "The pinned browser".into();
    }
    key.split(['-', '_', ' '])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// **Reason 1** — the pinned browser has been uninstalled. The URL still
/// opened, in the OS default browser: `"⚠️ Brave is gone — opened in Edge"`.
///
/// `default_browser_stem` is `browser_stem()`'s answer, or `None` when the
/// registry could not name one. It is NOT guessed at: saying "opened in Edge"
/// when it might have been Chrome is exactly the vagueness this replaces, so
/// the unresolved case says "your default browser" instead of a name.
pub fn pinned_browser_missing_toast(pinned_exe: &str, default_browser_stem: Option<&str>) -> String {
    let gone = display_name_for_exe(pinned_exe);
    match default_browser_stem
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(stem) => format!("⚠️ {gone} is gone — opened in {}", display_name_for_stem(stem)),
        None => format!("⚠️ {gone} is gone — opened in your default browser"),
    }
}

/// **Reason 2** — the pinned PROFILE folder was deleted inside the browser.
/// That browser still opened, into its own default profile:
/// `"⚠️ STUDIES is gone — Brave opened"`.
///
/// The profile is named from `browser_profile_name`, stored at pick time and
/// the whole reason that field exists (the handoff: never re-read the
/// browser's `Local State` on the Space-hold path). When it is missing or
/// blank the FOLDER name is used verbatim — `Profile 7`, `Default` — which is
/// the handoff's rule for a profile with no name, and is what the user would
/// see in the browser itself.
pub fn stale_profile_toast(
    profile_name: Option<&str>,
    profile_dir: &str,
    browser_exe: &str,
) -> String {
    let profile = profile_name
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| profile_dir.trim());
    format!(
        "⚠️ {profile} is gone — {} opened",
        display_name_for_exe(browser_exe)
    )
}

/// Real-filesystem existence check.
///
/// `Path::exists()` alone is not enough here. An MSIX app-execution alias
/// (`%LOCALAPPDATA%\Microsoft\WindowsApps\Arc.exe`) is a 0-byte reparse point
/// with the `IO_REPARSE_TAG_APPEXECLINK` tag, and `fs::metadata` — which
/// `Path::exists()` uses — FOLLOWS reparse points and can fail on that tag with
/// os error 1920 even though the entry is right there and launches fine.
/// `symlink_metadata` does not follow, so it answers the question actually
/// being asked: is there an entry at this path?
pub fn path_exists(p: &Path) -> bool {
    p.exists() || std::fs::symlink_metadata(p).is_ok()
}

/// `%LOCALAPPDATA%` as a path, if the variable is set.
pub fn local_app_data() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
}

// ---------------------------------------------------------------------------
// The scan
// ---------------------------------------------------------------------------

/// How deep under `%LOCALAPPDATA%` / `%APPDATA%` to look for a `Local State`.
///
/// MEASURED, NOT GUESSED (2026-08-26, this machine). Chrome, Edge, Brave and
/// Samsung all sit at depth 4 (`<vendor>\<product>\User Data\Local State`).
/// Arc is the outlier at depth SIX, because it is MSIX-packaged and its data is
/// redirected through
/// `Packages\TheBrowserCompany.Arc_…\LocalCache\Local\Arc\User Data`. 7 is that
/// worst real case plus one level of margin — enough for a packaged browser
/// nested one deeper, and far short of walking AppData unboundedly.
const MAX_SCAN_DEPTH: usize = 7;

/// Directory names never descended into.
///
/// This is a PERFORMANCE guard, not a correctness one — every one of these is
/// either a browser's own cache (thousands of files, and any `Local State`
/// inside one belongs to a browser already found at its parent) or a
/// well-known non-browser data folder. Measured effect on this machine: the
/// walk drops from 13.8s to 3.0s, and the candidate list from 66 to 18.
///
/// `EBWebView` earns its place twice over: it is the WebView2 runtime's data
/// folder, there are roughly forty of them under AppData here, and every single
/// one contains a `Local State` carrying a `profile.info_cache` — the exact
/// shape this scan looks for.
const SKIP_DIRS: &[&str] = &[
    "Cache", "Code Cache", "GPUCache", "GrShaderCache", "GraphiteDawnCache",
    "ShaderCache", "DawnCache", "DawnGraphiteCache", "DawnWebGPUCache",
    "Crashpad", "Service Worker", "IndexedDB", "Local Storage", "Session Storage",
    "blob_storage", "Extensions", "Extension Rules", "Extension State",
    "Sync Data", "File System", "databases", "Storage", "WebStorage",
    "Media Cache", "Application Cache", "Network", "Safe Browsing",
    "EBWebView", "CefCache", "htmlcache", "WebView2",
    "Temp", "node_modules", ".git", "Logs", "logs", "Sessions", "History",
    "Backup", "Package Cache", "Downloaded Installations", "INetCache",
    "Publishers", "SquirrelTemp", "pip", "npm-cache", "D3DSCache",
    "VirtualStore", "CrashDumps", "ConnectedDevicesPlatform",
];

fn is_skipped(name: &str) -> bool {
    // `Profile 1`, `Profile 12`… are a browser's own profile folders. The
    // `Local State` that describes them is at the level ABOVE, already found.
    if name.starts_with("Profile ") || name == "System Profile" || name == "Guest Profile" {
        return true;
    }
    SKIP_DIRS.iter().any(|s| s.eq_ignore_ascii_case(name))
}

/// Collect directories that directly contain a file named `Local State`.
///
/// Stops descending as soon as one is found: a `Local State` nested inside a
/// browser's own data tree is that browser's, not a second browser.
fn find_local_state_dirs(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if out.len() > 400 {
        return; // pathological tree; the real count here is 18
    }
    if dir.join("Local State").is_file() {
        out.push(dir.to_path_buf());
        return;
    }
    if depth >= MAX_SCAN_DEPTH {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for entry in rd.flatten() {
        // `file_type()` does not follow links, so a junction loop cannot hang
        // the walk.
        let Ok(ft) = entry.file_type() else { continue };
        if !ft.is_dir() {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if is_skipped(&name) {
            continue;
        }
        find_local_state_dirs(&entry.path(), depth + 1, out);
    }
}

/// Parse `info_cache` into profiles, keeping only those whose folder still
/// exists beside the `Local State`.
///
/// `Err` carries WHY the candidate failed the shape check, so the scan can log
/// it (owner's decision 2026-08-26: "log clearly when a browser is skipped").
/// Before this, a `Local State` that failed here vanished silently, and a
/// friend's "Opera doesn't show up" was undiagnosable from the log.
fn profiles_from_local_state(user_data: &Path) -> Result<Vec<BrowserProfile>, &'static str> {
    let Ok(text) = std::fs::read_to_string(user_data.join("Local State")) else {
        return Err("its Local State could not be read");
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Err("its Local State is not valid JSON");
    };
    let Some(cache) = json
        .get("profile")
        .and_then(|p| p.get("info_cache"))
        .and_then(|c| c.as_object())
    else {
        return Err("its Local State has no profile.info_cache (not the Chromium browser shape)");
    };
    if cache.is_empty() {
        return Err("its profile.info_cache is empty");
    }

    let mut out: Vec<BrowserProfile> = cache
        .iter()
        // A profile listed in info_cache whose folder is gone is a stale entry
        // in the browser's own bookkeeping. Offering it would let the user pick
        // something that cannot work.
        .filter(|(dir, _)| user_data.join(dir).is_dir())
        .map(|(dir, v)| {
            let display = v
                .get("name")
                .and_then(|n| n.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or(dir)
                .to_string();
            // `user_name` only — see the doc comment on `BrowserProfile::email`
            // for why `gaia_name` is not a usable fallback. `filter` collapses
            // BOTH "present but empty" (Edge/Brave/Samsung, measured) and a
            // trim-to-nothing value to `None`, same as `display` above.
            let email = v
                .get("user_name")
                .and_then(|n| n.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            // Derived once, here, so every surface renders the SAME string.
            let account_label = account_label(&display, email.as_deref());
            BrowserProfile {
                directory: dir.clone(),
                display_name: display,
                email,
                account_label,
            }
        })
        .collect();

    if out.is_empty() {
        return Err("its info_cache lists profiles, but none of their folders exist beside Local State");
    }
    // "Default" is the profile the browser opens on its own, so it leads.
    out.sort_by(|a, b| {
        let rank = |p: &BrowserProfile| if p.directory == "Default" { 0 } else { 1 };
        // Sorted by what the user SEES (`account_label`), not by
        // `display_name`, which is no longer the headline on any surface. A
        // list ordered by an invisible key reads as not sorted at all.
        rank(a)
            .cmp(&rank(b))
            .then_with(|| a.account_label.to_lowercase().cmp(&b.account_label.to_lowercase()))
    });
    Ok(out)
}

/// Which of the two real-world user-data layouts a candidate has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataLayout {
    /// `<product>\User Data\Local State` — Chrome, Edge, Brave, Samsung, Arc
    /// (all measured on this machine) and, per its documentation, Vivaldi. The
    /// product folder is the PARENT of `User Data`.
    NestedUserData,
    /// `<data dir>\Local State` directly, no `User Data` level. Opera Stable /
    /// Opera GX per their documented layout (REASONED — not installed here),
    /// and also the shape most EMBEDDED Chromium/CEF data dirs take: Spotify's
    /// `%LOCALAPPDATA%\Spotify\Local State` (measured 2026-08-26) sits exactly
    /// like this, complete with an info_cache listing `Browser` and `Default`.
    /// The data dir itself is the closest thing there is to a product folder.
    SelfContained,
}

/// The product folder for a user-data directory, and which layout it has.
///
/// Before 2026-08-26 this was an inline `user_data.parent()`, which silently
/// bakes in the `<product>\User Data` assumption — for an Opera-shaped dir it
/// produced `%APPDATA%\Opera Software` (the VENDOR folder), which no resolution
/// source could ever match, so Opera was undetectable by construction.
pub fn product_dir_for(user_data: &Path) -> Option<(PathBuf, DataLayout)> {
    if user_data
        .file_name()
        .is_some_and(|n| n.eq_ignore_ascii_case("User Data"))
    {
        user_data
            .parent()
            .map(|p| (p.to_path_buf(), DataLayout::NestedUserData))
    } else {
        Some((user_data.to_path_buf(), DataLayout::SelfContained))
    }
}

/// Part of "log clearly when a browser is skipped": did this candidate LOOK
/// like a real browser before exe resolution failed on it?
///
/// Deliberately conservative, per the owner's decision. Plausible means BOTH:
///   · its `info_cache` listed at least one profile whose folder exists (the
///     scan only calls this after the shape check passed, so that is already
///     true — the parameter keeps the function honest on its own), AND
///   · no component of its path names a known embedded-webview data dir.
///
/// The embedded-marker list is belt-and-braces: `SKIP_DIRS` already prunes
/// `EBWebView` (and friends) out of the walk, so those ~40 candidates never
/// reach this function today — but a future edit to SKIP_DIRS must not silently
/// promote forty WebView2 hosts to info-level log lines.
fn looks_like_plausible_browser(user_data: &Path, profiles: &[BrowserProfile]) -> bool {
    if profiles.is_empty() {
        return false;
    }
    const EMBEDDED_MARKERS: &[&str] = &["EBWebView", "WebView2", "CefCache", "htmlcache"];
    !user_data.components().any(|c| match c {
        std::path::Component::Normal(s) => {
            let s = s.to_string_lossy();
            EMBEDDED_MARKERS.iter().any(|m| s.eq_ignore_ascii_case(m))
        }
        _ => false,
    })
}

/// Executables that live beside a browser but are not the browser.
fn is_helper_exe(stem_lower: &str) -> bool {
    const BAD: &[&str] = &[
        "updater", "update", "helper", "crashpad", "crash", "proxy", "setup",
        "install", "uninstall", "notification", "elevat", "reporter",
    ];
    BAD.iter().any(|b| stem_lower.contains(b))
}

/// Source 1 — a per-user install that keeps its exe under the product folder.
///
/// Direct children of `Application` only. The versioned subfolder beside them
/// (`Application\147.1.89.132\`) is the copy that gets replaced on every
/// update, so binding to it would break exactly the way PROBLEM 116 describes.
fn exe_from_application_dir(product: &Path) -> Option<String> {
    let app_dir = product.join("Application");
    let rd = std::fs::read_dir(&app_dir).ok()?;
    let product_name = product.file_name()?.to_string_lossy().to_lowercase();

    let mut candidates: Vec<PathBuf> = Vec::new();
    for e in rd.flatten() {
        let p = e.path();
        if !p.is_file() || !p.extension().is_some_and(|x| x.eq_ignore_ascii_case("exe")) {
            continue;
        }
        let stem = p.file_stem()?.to_string_lossy().to_lowercase();
        if is_helper_exe(&stem) {
            continue;
        }
        candidates.push(p);
    }

    match candidates.len() {
        0 => None,
        1 => Some(candidates.remove(0).to_string_lossy().into_owned()),
        _ => {
            // Several plausible exes. Prefer one whose name relates to the
            // product folder ("Brave-Browser" -> brave.exe); if none does,
            // refuse rather than pick arbitrarily.
            let prod_key: String = product_name.chars().filter(|c| c.is_alphanumeric()).collect();
            candidates
                .into_iter()
                .find(|p| {
                    let stem: String = p
                        .file_stem()
                        .map(|s| s.to_string_lossy().to_lowercase())
                        .unwrap_or_default()
                        .chars()
                        .filter(|c| c.is_alphanumeric())
                        .collect();
                    !stem.is_empty()
                        && (prod_key.contains(&stem) || stem.contains(&prod_key))
                })
                .map(|p| p.to_string_lossy().into_owned())
        }
    }
}

/// One browser Windows knows about, from `Clients\StartMenuInternet`.
struct RegisteredBrowser {
    /// Windows' own human name for it ("Google Chrome", "Brave"). The key's
    /// DEFAULT VALUE when it has one, falling back to the key name — the key
    /// name is not always human: measured on this machine, IE's key is
    /// "IEXPLORE.EXE" (default value "Internet Explorer") and Perplexity's is
    /// "Comet.GNZHVZYZL3BMQQFOFFCFNEZIYI" (default value "Comet"); Opera's is
    /// documented as "OperaStable". For every browser that resolved through
    /// this source before the change (Chrome, Brave, Edge, Samsung), the
    /// default value IS the key name — measured — so nothing shown today
    /// changes.
    name: String,
    exe: String,
    /// The exe's product folder — the parent of `Application` when the exe
    /// sits inside one, otherwise the exe's own directory (Opera's
    /// `…\Programs\Opera\opera.exe` -> `…\Programs\Opera`).
    product: PathBuf,
}

/// Source 2 — every browser registered as a default-browser candidate.
///
/// `HKLM\SOFTWARE\Clients\StartMenuInternet` (plus the HKCU copy, where
/// per-user installs land) is Windows' own generic "I am a web browser"
/// declaration. Using it keeps this vendor-agnostic while giving each browser a
/// properly capitalised human name for free.
///
/// Until 2026-08-26 entries whose exe was NOT inside a folder called
/// `Application` were dropped. That was how Internet Explorer
/// (`Internet Explorer\iexplore.exe`) was excluded — but it also excluded
/// Opera, whose exe sits directly in `…\Programs\Opera\`, so the one browser a
/// friend actually reported missing was being filtered by the guard against a
/// browser nobody has used in years. Non-`Application` entries are now KEPT,
/// with the exe's own directory as the product folder. IE (and Firefox, and
/// anything else non-Chromium that registers here) is still excluded, by the
/// mechanism that was doing the real work all along: a registered browser only
/// ever REACHES the picker by marrying a Chromium-shaped data dir via
/// `product_paths_match`, and non-Chromium browsers have none to marry.
#[cfg(windows)]
fn registered_browsers() -> Vec<RegisteredBrowser> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;

    let mut out = Vec::new();
    for root in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
        let Ok(clients) = RegKey::predef(root).open_subkey(r"SOFTWARE\Clients\StartMenuInternet")
        else {
            continue;
        };
        for name in clients.enum_keys().flatten() {
            let Ok(cmd_key) = clients.open_subkey(format!(r"{name}\shell\open\command")) else {
                continue;
            };
            let Ok(cmd) = cmd_key.get_value::<String, _>("") else { continue };
            let Some(exe) = exe_from_command(&cmd) else { continue };
            let p = PathBuf::from(&exe);
            let Some(exe_dir) = p.parent() else { continue };
            let in_application = exe_dir
                .file_name()
                .is_some_and(|n| n.eq_ignore_ascii_case("Application"));
            let product = if in_application {
                match exe_dir.parent() {
                    Some(prod) => prod.to_path_buf(),
                    None => continue,
                }
            } else {
                exe_dir.to_path_buf()
            };
            // The registered key's default value is the display name Windows
            // itself shows for this browser. Blank/missing falls back to the
            // key name, which is what this code always used.
            let display = clients
                .open_subkey(&name)
                .and_then(|k| k.get_value::<String, _>(""))
                .ok()
                .map(|s: String| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| name.clone());
            out.push(RegisteredBrowser { name: display, exe, product });
        }
    }
    out
}

#[cfg(not(windows))]
fn registered_browsers() -> Vec<RegisteredBrowser> {
    Vec::new()
}

/// Pull the executable out of a registry `shell\open\command` value, which is
/// quoted (`"C:\…\brave.exe"`) or bare (`C:\…\iexplore.exe`), sometimes with
/// trailing arguments.
pub fn exe_from_command(cmd: &str) -> Option<String> {
    let cmd = cmd.trim();
    if let Some(rest) = cmd.strip_prefix('"') {
        let end = rest.find('"')?;
        return Some(rest[..end].to_string());
    }
    // Unquoted: take everything up to and including the first ".exe".
    let lower = cmd.to_lowercase();
    let idx = lower.find(".exe")? + 4;
    Some(cmd[..idx].to_string())
}

/// Path components that are containers rather than part of a product's
/// identity. Dropping them is what lets an INSTALL directory be compared with a
/// DATA directory at all: `Program Files\Google\Chrome` and
/// `AppData\Local\Google\Chrome` name the same product and share not one
/// leading component.
const GENERIC_PATH_PARTS: &[&str] = &[
    "program files", "program files (x86)", "programdata",
    "appdata", "local", "locallow", "roaming", "programs",
    "packages", "localcache", "windowsapps",
];

/// The identity-bearing components of a path, lowercased.
fn meaningful_parts(p: &Path) -> Vec<String> {
    let mut comps: Vec<String> = p
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(s) => Some(s.to_string_lossy().to_lowercase()),
            _ => None,
        })
        .collect();
    // `C:\Users\<name>\…` — the leading pair is container identity, not product
    // identity, and the username cannot be filtered by the static list above.
    // Without dropping it, a per-user install that does not nest under a vendor
    // folder (`…\AppData\Local\Programs\Opera`) reads as
    // ["users", "<name>", "opera"], its 3-component length forces the
    // two-component vendor comparison, and "<name>" is compared against a real
    // vendor — so the marriage fails for EVERY unnested per-user install. The
    // one-component fallback below was always the intended semantics for those.
    if comps.len() >= 2 && comps[0] == "users" {
        comps.drain(..2);
    }
    comps.retain(|s| !GENERIC_PATH_PARTS.contains(&s.as_str()));
    comps
}

/// Do two paths name the same product?
///
/// This is what marries a browser's INSTALL directory to its LOCALAPPDATA DATA
/// directory — `…\Google\Chrome\Application\chrome.exe` to
/// `…\Local\Google\Chrome` — which is the only link between the two on this
/// machine, since nothing inside `Local State` records where its browser is
/// installed.
///
/// Two components are compared, not one, and the fallback to one is
/// CONDITIONAL. A bare leaf match is genuinely dangerous: Samsung's browser
/// lives in a folder called `Internet`, and any other vendor shipping an
/// `Internet` folder would have been silently married to Samsung's profiles —
/// a picker entry that opens the wrong program, which is the one failure this
/// module is supposed to make impossible. So the leaf-only comparison is
/// allowed ONLY when a side has no second identity component to offer (an
/// install like `C:\Program Files\Vivaldi` that does not nest under a vendor
/// folder). When both sides have two, both must match.
/// `"opera stable"` -> `"opera"`. Opera's channel decoration appears in its
/// DATA folder's name but not its INSTALL folder's: the data dirs are
/// documented as "Opera Stable" / "Opera GX Stable" while the install dirs are
/// "Opera" / "Opera GX" (REASONED — Opera is not installed on this machine to
/// measure; see the Opera note in the module header). Only a trailing
/// " stable" is stripped, and nothing else: Chrome's own channels decorate
/// BOTH sides identically ("Chrome Beta" data AND install), so they never need
/// this, and a looser rule would start joining products that merely share a
/// prefix. Input is already lowercased by `meaningful_parts`.
fn strip_channel_suffix(leaf: &str) -> &str {
    leaf.strip_suffix(" stable").unwrap_or(leaf)
}

/// Leaf comparison for `product_paths_match`, tolerant of the channel suffix
/// on either side (symmetric, so it cannot matter which side is the install
/// and which the data directory).
fn leaves_match(a: &str, b: &str) -> bool {
    a == b || strip_channel_suffix(a) == strip_channel_suffix(b)
}

pub fn product_paths_match(a: &Path, b: &Path) -> bool {
    let (pa, pb) = (meaningful_parts(a), meaningful_parts(b));
    let (Some(la), Some(lb)) = (pa.last(), pb.last()) else { return false };
    if !leaves_match(la, lb) {
        return false;
    }
    // Both sides carry a vendor component: it has to agree.
    if pa.len() >= 2 && pb.len() >= 2 {
        return pa[pa.len() - 2] == pb[pb.len() - 2];
    }
    // One side is a bare product folder — the leaf is all there is to go on.
    true
}

/// Source 3 — an MSIX app-execution alias whose name matches the product
/// folder, which is how a packaged browser (Arc) is reachable at all: its real
/// exe lives under `WindowsApps` behind an ACL that forbids launching it by
/// path, while the alias is the supported entry point.
///
/// Deliberately an EXACT `<product folder name>.exe` match. `Arc` -> `Arc.exe`
/// is unambiguous; anything looser would start matching unrelated aliases
/// (there are 43 of them on this machine, including `python.exe` and
/// `notepad.exe`).
#[cfg(windows)]
fn exe_from_app_alias(product: &Path) -> Option<String> {
    let name = product.file_name()?.to_string_lossy().into_owned();
    let alias = local_app_data()?
        .join(r"Microsoft\WindowsApps")
        .join(format!("{name}.exe"));
    if path_exists(&alias) {
        Some(alias.to_string_lossy().into_owned())
    } else {
        None
    }
}

#[cfg(not(windows))]
fn exe_from_app_alias(_product: &Path) -> Option<String> {
    None
}

/// `"Brave-Browser"` -> `"Brave Browser"`. Only used when Windows has no name
/// of its own for this browser.
fn humanise(folder: &str) -> String {
    folder.replace(['-', '_'], " ").trim().to_string()
}

/// Scan for Chromium browsers and their profiles. Takes a few seconds; callers
/// go through `list_browser_profiles`, which caches.
pub fn scan_browsers() -> Vec<DetectedBrowser> {
    let mut roots: Vec<PathBuf> = Vec::new();
    for var in ["LOCALAPPDATA", "APPDATA"] {
        if let Some(v) = std::env::var_os(var) {
            roots.push(PathBuf::from(v));
        }
    }

    let mut candidates: Vec<PathBuf> = Vec::new();
    for r in &roots {
        find_local_state_dirs(r, 0, &mut candidates);
    }

    let registered = registered_browsers();
    let mut out: Vec<DetectedBrowser> = Vec::new();

    for user_data in candidates {
        let local_state = user_data.join("Local State");

        // Shape check first — it is a cheap read that rejects most candidates,
        // though on its own it rejects far less than one would hope (see the
        // header note about EBWebView). Shape failures log at debug!, never
        // info!: they are the ORDINARY outcome (Electron apps, half-empty CEF
        // dirs), and the shipped log filters debug out, so this cannot spam it.
        let profiles = match profiles_from_local_state(&user_data) {
            Ok(p) => p,
            Err(why) => {
                log::debug!("browser_profiles: skipped {} — {why}", local_state.display());
                continue;
            }
        };
        let Some((product, layout)) = product_dir_for(&user_data) else {
            log::debug!(
                "browser_profiles: skipped {} — no product folder is derivable for it",
                local_state.display()
            );
            continue;
        };

        // --- resolve an executable, or drop this candidate entirely ---
        let mut name: Option<String> = None;
        let exe = exe_from_application_dir(&product)
            .or_else(|| {
                registered
                    .iter()
                    .find(|r| product_paths_match(&r.product, &product))
                    .map(|r| {
                        name = Some(r.name.clone());
                        r.exe.clone()
                    })
            })
            .or_else(|| {
                // The MSIX-alias source exists for Arc's layout, which nests
                // under `User Data` — and it is DELIBERATELY not consulted for
                // a self-contained data dir. For those, the product folder IS
                // the data dir, so any embedded-CEF app whose data folder
                // happens to share a name with an unrelated alias would marry
                // it: Spotify's `%LOCALAPPDATA%\Spotify` (measured, shape check
                // passes) + the Store Spotify's `Spotify.exe` alias would put
                // Spotify in the browser picker.
                if layout == DataLayout::NestedUserData {
                    exe_from_app_alias(&product)
                } else {
                    None
                }
            });

        let Some(exe) = exe else {
            // Owner's decision 2026-08-26: "log clearly when a browser is
            // skipped". A candidate that got THIS far parsed as a browser and
            // listed live profiles — only the exe hunt failed. That is the
            // intended fate of embedded webviews (it is what keeps WebView2
            // data folders, including Spaceadom's own, out of the picker), but
            // it is also exactly where a REAL browser this code has not met
            // would disappear, so a plausible-looking one gets ONE findable
            // info! line and the noise stays at debug!.
            let msg = format!(
                "browser_profiles: skipped {} — its Local State parses and lists {} usable \
                 profile(s), but no launcher exe could be resolved for it (no exe in \
                 {}\\Application, no Clients\\StartMenuInternet registration matching that \
                 product folder, no matching MSIX app alias)",
                local_state.display(),
                profiles.len(),
                product.display(),
            );
            if looks_like_plausible_browser(&user_data, &profiles) {
                log::info!(
                    "{msg}. If a real browser is missing from the profile picker, this line is why."
                );
            } else {
                log::debug!("{msg}.");
            }
            continue;
        };
        if !path_exists(Path::new(&exe)) {
            // A resolution source ANSWERED and the answer is a dead path — a
            // stale registry entry or a half-uninstalled browser. Rarer and
            // stranger than the case above, so it is always worth an info line.
            log::info!(
                "browser_profiles: skipped {} — an exe was resolved for it ({exe}) but \
                 nothing exists at that path (stale registration?)",
                local_state.display()
            );
            continue;
        }

        let browser_name = name.unwrap_or_else(|| {
            humanise(&product.file_name().unwrap_or_default().to_string_lossy())
        });

        // Duplicate exes can arrive from two candidate paths (a browser with a
        // second data directory). First one wins.
        if out.iter().any(|b: &DetectedBrowser| b.browser_exe.eq_ignore_ascii_case(&exe)) {
            continue;
        }

        out.push(DetectedBrowser {
            browser_name,
            browser_exe: exe,
            user_data_dir: user_data.to_string_lossy().into_owned(),
            icon_base64: None, // filled by the command, which owns the cache
            profiles,
        });
    }

    out.sort_by(|a, b| a.browser_name.to_lowercase().cmp(&b.browser_name.to_lowercase()));
    out
}

// ---------------------------------------------------------------------------
// The Tauri command
// ---------------------------------------------------------------------------

/// Session cache. The scan takes ~3s on this machine, and nothing it looks at
/// changes while the app is running in any way the user could act on.
static SCAN_CACHE: std::sync::OnceLock<std::sync::Mutex<Option<Vec<DetectedBrowser>>>> =
    std::sync::OnceLock::new();

/// Detected Chromium browsers with their profiles, for the key editor's
/// profile picker. Cached for the session.
///
/// Icons come from the SAME extractor and the SAME `IconCacheState` as
/// `list_start_menu_apps`, so a browser already drawn in the app grid costs
/// nothing to draw again here.
///
/// PROBLEM 237 — `async`, on a `spawn_blocking` thread with its own balanced
/// STA (`icon_extractor::ComSta`): the AppData walk is ~2.5 s and the per-
/// browser icon is in-process COM, and both used to run on the MAIN THREAD as
/// one of the three commands `warmPickerData()` fires together. `Result`
/// because Tauri demands it of an async command borrowing `State<'_>`; the
/// frontend's `invoke<DetectedBrowser[]>` is unaffected (the promise resolves
/// with the `Ok` value, and its `.catch` already exists).
#[tauri::command]
pub async fn list_browser_profiles(
    cache: tauri::State<'_, crate::commands::IconCacheState>,
) -> Result<Vec<DetectedBrowser>, String> {
    let cache = std::sync::Arc::clone(&cache.0);
    tauri::async_runtime::spawn_blocking(move || {
        let _com = crate::icon_extractor::ComSta::new();
        list_browser_profiles_blocking(&cache)
    })
    .await
    .map_err(|e| format!("browser scan could not run: {e}"))
}

/// The body of `list_browser_profiles`, on whatever thread the caller chose.
fn list_browser_profiles_blocking(
    cache: &std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, String>>>,
) -> Vec<DetectedBrowser> {
    let slot = SCAN_CACHE.get_or_init(|| std::sync::Mutex::new(None));
    let mut guard = slot.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(cached) = guard.as_ref() {
        return cached.clone();
    }

    let started = std::time::Instant::now();
    let mut found = scan_browsers();

    for b in &mut found {
        let mut lock = cache.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(hit) = lock.get(&b.browser_exe) {
            b.icon_base64 = Some(hit.clone());
        } else if let Some(b64) = crate::icon_extractor::extract_icon(&b.browser_exe) {
            lock.insert(b.browser_exe.clone(), b64.clone());
            b.icon_base64 = Some(b64);
        }
    }

    log::info!(
        "browser_profiles: found {} Chromium browser(s) in {}ms on thread {} — {}",
        found.len(),
        started.elapsed().as_millis(),
        crate::picker_worker::os_thread_id(),
        found
            .iter()
            .map(|b| format!("{} ({} profile(s))", b.browser_name, b.profiles.len()))
            .collect::<Vec<_>>()
            .join(", ")
    );

    // 1.0.95 — the account-label pass, and the ONLY line that reports it.
    //
    // COUNTS ONLY. Neither an address nor its local part may appear in a log
    // line: `debug.log` is quoted into bug reports and a panic in this process
    // ships its message to Sentry. What is actually diagnosable is the SPLIT —
    // "0 of 14 labelled by account" says the `user_name` read came back empty
    // on a machine where it should not have, which is the failure this feature
    // can plausibly have, and it says it without naming anybody.
    let (signed_in, total) = found.iter().flat_map(|b| b.profiles.iter()).fold(
        (0usize, 0usize),
        |(s, t), p| (s + usize::from(p.email.is_some()), t + 1),
    );
    log::info!(
        "browser_profiles: account labels resolved — {signed_in} of {total} profile(s) are \
         signed in and are labelled by the local part of their account; the remaining \
         {} keep the browser's own display name (no address is ever logged)",
        total - signed_in
    );

    *guard = Some(found.clone());
    found
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod hard_requirement_tests {
    use super::*;
    use crate::config::KeyBinding;

    fn bare() -> KeyBinding {
        KeyBinding {
            app: None,
            web_url: Some("https://example.com".into()),
            label: Some("Example".into()),
            icon_override: None,
            browser_exe: None,
            browser_profile_dir: None,
            browser_profile_name: None,
            // PROBLEM 267 — a link icon is fetched by the editor, never seeded.
            site_icon: None,
        }
    }

    /// Nothing exists. Used to prove a decision does not secretly depend on the
    /// filesystem.
    fn nothing_exists(_: &Path) -> bool {
        false
    }
    fn everything_exists(_: &Path) -> bool {
        true
    }

    /// **THE HARD REQUIREMENT.** The owner, twice: *"MAKE SURE THE DEFAULT
    /// BROWSER LAUNCHES FROM URL IF NOT EXPLICITLY SET TO SPECIFIC."*
    ///
    /// Every binding that exists today, and every new one where the user never
    /// touches the profile picker, must take the untouched `run_browser` path.
    #[test]
    fn a_binding_with_no_browser_exe_uses_the_default_browser() {
        let b = bare();
        assert!(
            !should_use_specific_browser(&b),
            "a binding with browser_exe: None must NOT use a specific browser"
        );
        assert_eq!(
            route_for(&b, &everything_exists),
            BrowserRoute::Default,
            "browser_exe: None must route to the DEFAULT browser even when every \
             path on the machine exists — nothing about the filesystem may \
             divert a binding the user never pinned"
        );
    }

    /// The blank-string case. A frontend writing `""` instead of `null` is an
    /// ordinary bug; the right answer to it is still the default browser.
    #[test]
    fn a_blank_browser_exe_is_treated_as_unset() {
        for blank in ["", "   ", "\t"] {
            let mut b = bare();
            b.browser_exe = Some(blank.into());
            assert!(
                !should_use_specific_browser(&b),
                "browser_exe {blank:?} must count as unset"
            );
            assert_eq!(route_for(&b, &everything_exists), BrowserRoute::Default);
        }
    }

    /// The other half: an explicitly pinned browser that IS present must take
    /// the new path. Without this, the test above could pass with the feature
    /// entirely disabled.
    #[test]
    fn an_explicitly_pinned_browser_uses_the_specific_path() {
        let mut b = bare();
        b.browser_exe = Some(r"C:\Program Files\Google\Chrome\Application\chrome.exe".into());
        assert!(should_use_specific_browser(&b));
        assert_eq!(
            route_for(&b, &everything_exists),
            BrowserRoute::Specific {
                exe: r"C:\Program Files\Google\Chrome\Application\chrome.exe".into()
            }
        );
    }

    /// A pinned browser that has since been UNINSTALLED falls back to the
    /// default rather than leaving a dead key.
    #[test]
    fn a_pinned_browser_that_is_gone_falls_back_instead_of_dying() {
        let mut b = bare();
        b.browser_exe = Some(r"C:\Gone\Application\ghost.exe".into());
        assert_eq!(
            route_for(&b, &nothing_exists),
            BrowserRoute::PinnedBrowserMissing { exe: r"C:\Gone\Application\ghost.exe".into() },
            "an uninstalled pinned browser must be a NAMED fallback, so the log \
             can say what happened rather than behaving like an unpinned key"
        );
    }

    /// **The dispatch site must actually CONSULT the guard**, not merely have a
    /// correct one available. A guard nobody calls passes every value-level test
    /// above and still ships the regression, so this asserts a property of the
    /// CALL SITE — something no test of this module's own functions can see.
    ///
    /// Reading the source is unusual and is the point. The property being
    /// protected is structural: "the decision happens, and it happens first".
    fn body_of(src: &str, signature: &str) -> String {
        // smart_cascade.rs is CRLF on disk (checked — 2263 of them), so the
        // "\n}\n" end marker below would never match against the raw bytes.
        // Normalising first is what makes this test about the CODE rather than
        // about whichever editor last touched the file; the first version of
        // this helper did not, and failed for exactly that reason.
        let src = src.replace("\r\n", "\n");
        let start = src
            .find(signature)
            .unwrap_or_else(|| panic!("{signature} must still exist in smart_cascade.rs"));
        // A Rust function at file scope ends at the first line that is exactly
        // "}" — every nested block inside it is indented.
        let end = src[start..]
            .find("\n}\n")
            .unwrap_or_else(|| panic!("{signature} must have a closing brace"));
        src[start..start + end].to_string()
    }

    #[test]
    fn the_url_dispatcher_consults_the_guard_before_choosing_a_path() {
        let src = include_str!("engine/actions/smart_cascade.rs");
        let body = body_of(src, "fn open_binding_url(");

        let route_at = body
            .find("route_for(")
            .expect("open_binding_url must call browser_profiles::route_for");
        let run_browser_at = body
            .find("run_browser(")
            .expect("open_binding_url must still call run_browser for the default path");
        assert!(
            route_at < run_browser_at,
            "the route decision must be made BEFORE the first run_browser call — \
             otherwise the default path is taken unconditionally and the pinned \
             one is dead code, or worse, the reverse"
        );
        assert!(
            body.contains("BrowserRoute::Default => return run_browser(url, app_handle)"),
            "the Default arm must call run_browser(url, app_handle) verbatim — same \
             function, same arguments, no new logic wrapped around it. That is the \
             owner's hard requirement, stated twice."
        );
    }

    /// There must be exactly ONE way for a URL binding to reach a browser, and
    /// it must be the guarded one. If `smart_cascade` could still call
    /// `run_browser` directly, a pinned binding would silently take the default
    /// path from whichever arm was forgotten — which is precisely how the
    /// primary and Founders-fallback arms of this function would drift.
    /// **Both fallback reasons must actually REACH the user**, not merely have
    /// a correct sentence available. Same reasoning as the guard test above: a
    /// message nobody emits passes every value-level test in this file and
    /// still ships the "something went wrong" experience the handoff is about.
    #[test]
    fn both_fallback_reasons_are_emitted_from_the_url_dispatcher() {
        let src = include_str!("engine/actions/smart_cascade.rs");
        let body = body_of(src, "fn open_binding_url(");

        assert!(
            body.contains("pinned_browser_missing_toast("),
            "the PinnedBrowserMissing arm must tell the user which browser vanished and \
             which one opened instead"
        );
        assert!(
            body.contains("stale_profile_toast("),
            "the DroppedStale arm must tell the user which PROFILE vanished — a different \
             sentence naming different things"
        );
        assert_eq!(
            body.matches("notify(&").count(),
            2,
            "exactly two, and both through the shared notify() — which reads the AppHandle \
             from the guide_hud OnceLock rather than adding a second copy of it"
        );
    }

    /// The app-binding path (a browser+profile bound with NO url) hits the very
    /// same stale-profile failure, and must say so too.
    #[test]
    fn the_app_path_also_reports_a_deleted_profile() {
        let src = include_str!("engine/actions/smart_cascade.rs");
        let body = body_of(src, "fn launch_binding_app(");
        assert!(
            body.contains("notify(&msg)"),
            "launch_binding_app must raise the stale-profile message"
        );
        let launched_at = body
            .find("if launched")
            .expect("the message must be gated on the launch having succeeded");
        let notify_at = body.find("notify(&msg)").unwrap();
        assert!(
            launched_at < notify_at,
            "'Brave opened' must only be said AFTER Brave has actually opened — a failed \
             launch already gets the cascade's own '❌ … could not be opened'"
        );
    }

    #[test]
    fn smart_cascade_has_no_unguarded_route_to_a_browser() {
        let src = include_str!("engine/actions/smart_cascade.rs");
        let body = body_of(src, "pub fn smart_cascade(");
        assert!(
            !body.contains("run_browser("),
            "smart_cascade must not call run_browser directly — every URL must go \
             through open_binding_url, which consults the guard"
        );
        assert!(
            body.contains("open_binding_url("),
            "smart_cascade must dispatch URLs through open_binding_url"
        );
    }
}

/// The design handoff, §5: *"Two different failures get two different
/// sentences. 'Something went wrong' leaves the user guessing which half to
/// fix."* These tests are that sentence, held to.
#[cfg(test)]
mod fallback_message_tests {
    use super::*;

    const BRAVE: &str = r"C:\Program Files\BraveSoftware\Brave-Browser\Application\brave.exe";

    #[test]
    fn the_two_reasons_are_different_sentences_naming_different_things() {
        let browser_gone = pinned_browser_missing_toast(BRAVE, Some("msedge"));
        let profile_gone = stale_profile_toast(Some("STUDIES"), "Profile 3", BRAVE);

        assert_eq!(browser_gone, "⚠️ Brave is gone — opened in Edge");
        assert_eq!(profile_gone, "⚠️ STUDIES is gone — Brave opened");
        assert_ne!(
            browser_gone, profile_gone,
            "the whole requirement: a user who reads one must know which half broke"
        );
        // The distinguishing detail, spelled out: reason 1 names the browser
        // that took over; reason 2 names the browser that still worked.
        assert!(browser_gone.contains("Edge") && !browser_gone.contains("STUDIES"));
        assert!(profile_gone.contains("STUDIES") && !profile_gone.contains("Edge"));
    }

    #[test]
    fn an_unresolvable_default_browser_is_not_guessed_at() {
        // No http/https handler registered, or its command line would not
        // parse. Naming a browser here would be the vagueness this replaces,
        // one step worse: a confidently WRONG name.
        assert_eq!(
            pinned_browser_missing_toast(BRAVE, None),
            "⚠️ Brave is gone — opened in your default browser"
        );
        assert_eq!(
            pinned_browser_missing_toast(BRAVE, Some("   ")),
            "⚠️ Brave is gone — opened in your default browser"
        );
    }

    #[test]
    fn a_profile_with_no_stored_name_falls_back_to_its_folder_verbatim() {
        // Handoff §5, "Profile with no name": `Profile 7`, `Default`,
        // `Person 1` — shown exactly as the browser shows them.
        for blank in [None, Some(""), Some("   ")] {
            assert_eq!(
                stale_profile_toast(blank, "Profile 7", BRAVE),
                "⚠️ Profile 7 is gone — Brave opened"
            );
        }
    }

    #[test]
    fn exe_stems_become_names_a_person_recognises() {
        // The only evidence left about an UNINSTALLED browser is the path the
        // binding stored, so this is the whole naming budget for reason 1.
        assert_eq!(display_name_for_exe(BRAVE), "Brave");
        assert_eq!(
            display_name_for_exe(r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe"),
            "Edge",
            "'Msedge' is not what anyone calls their browser"
        );
        assert_eq!(
            display_name_for_exe(r"C:\Program Files\Google\Chrome\Application\chrome.exe"),
            "Chrome"
        );
        // Unknown browsers must still produce something — no vendor list.
        assert_eq!(display_name_for_exe(r"D:\somewhere\ladybird.exe"), "Ladybird");
        assert_eq!(display_name_for_stem("opera_gx"), "Opera GX");
        // A stored path with no filename at all still yields a sentence.
        assert_eq!(display_name_for_exe(""), "The pinned browser");
    }

    /// The message paths must not be reachable from an UNPINNED binding — the
    /// hard requirement seen from the other side. A binding with no
    /// `browser_exe` routes to `Default`, which produces neither sentence.
    #[test]
    fn an_unpinned_binding_produces_no_fallback_message_at_all() {
        let b = crate::config::KeyBinding {
            app: None,
            web_url: Some("https://example.com".into()),
            label: None,
            icon_override: None,
            browser_exe: None,
            browser_profile_dir: None,
            browser_profile_name: None,
            site_icon: None,
        };
        assert_eq!(route_for(&b, &|_: &Path| false), BrowserRoute::Default);
    }
}

#[cfg(test)]
mod profile_arg_tests {
    use super::*;

    /// A fake filesystem: only these exact paths exist.
    fn only(paths: &'static [&'static str]) -> impl Fn(&Path) -> bool {
        move |p: &Path| {
            let s = p.to_string_lossy().to_lowercase();
            paths.iter().any(|q| q.to_lowercase() == s)
        }
    }

    /// No profile pinned — nothing to decide, and no argument produced.
    #[test]
    fn no_profile_means_no_argument() {
        let fs = only(&[]);
        assert_eq!(
            profile_arg_for(r"C:\B\Application\b.exe", None, None, &fs),
            ProfileArg::None
        );
        assert_eq!(
            profile_arg_for(r"C:\B\Application\b.exe", Some("  "), None, &fs),
            ProfileArg::None
        );
    }

    /// **THE STALE-PROFILE FALLBACK.** The user deleted "Profile 1" inside the
    /// browser. The `User Data` directory is found, the profile folder is not,
    /// so the argument is DROPPED and the caller warns.
    #[test]
    fn a_deleted_profile_folder_drops_the_argument() {
        // User Data exists; "Profile 1" inside it does not.
        let fs = only(&[r"C:\Users\me\AppData\Local\Google\Chrome\User Data"]);
        let lad = PathBuf::from(r"C:\Users\me\AppData\Local");

        let got = profile_arg_for(
            r"C:\Program Files\Google\Chrome\Application\chrome.exe",
            Some("Profile 1"),
            Some(&lad),
            &fs,
        );
        assert_eq!(
            got,
            ProfileArg::DroppedStale {
                dir: "Profile 1".into(),
                searched: PathBuf::from(r"C:\Users\me\AppData\Local\Google\Chrome\User Data"),
            },
            "a profile folder that is gone must drop --profile-directory so the \
             browser opens its own default, not fail to launch"
        );
    }

    /// The same layout with the profile folder PRESENT keeps the argument.
    /// Without this the test above would also pass if the code dropped the
    /// argument unconditionally.
    #[test]
    fn a_live_profile_folder_keeps_the_argument() {
        let fs = only(&[
            r"C:\Users\me\AppData\Local\Google\Chrome\User Data",
            r"C:\Users\me\AppData\Local\Google\Chrome\User Data\Profile 1",
        ]);
        let lad = PathBuf::from(r"C:\Users\me\AppData\Local");
        assert_eq!(
            profile_arg_for(
                r"C:\Program Files\Google\Chrome\Application\chrome.exe",
                Some("Profile 1"),
                Some(&lad),
                &fs,
            ),
            ProfileArg::Use("Profile 1".into())
        );
    }

    /// A per-user install keeps exe and profiles under one product folder, and
    /// that candidate is tried FIRST — no LOCALAPPDATA needed.
    #[test]
    fn a_per_user_install_resolves_without_localappdata() {
        let fs = only(&[
            r"C:\Users\me\AppData\Local\Vendor\Fork\User Data",
            r"C:\Users\me\AppData\Local\Vendor\Fork\User Data\Profile 3",
        ]);
        assert_eq!(
            profile_arg_for(
                r"C:\Users\me\AppData\Local\Vendor\Fork\Application\fork.exe",
                Some("Profile 3"),
                None,
                &fs,
            ),
            ProfileArg::Use("Profile 3".into())
        );
    }

    /// **The polarity that matters most.** When the `User Data` directory
    /// cannot be located at all — an MSIX-packaged browser like Arc, or any
    /// layout this code has not met — the argument is KEPT.
    ///
    /// "Could not verify" is not "verified absent". Dropping here would silently
    /// break a launch that works.
    #[test]
    fn an_unlocatable_user_data_dir_keeps_the_argument() {
        let fs = only(&[]); // nothing resolves
        let lad = PathBuf::from(r"C:\Users\me\AppData\Local");
        assert_eq!(
            profile_arg_for(
                r"C:\Users\me\AppData\Local\Microsoft\WindowsApps\Arc.exe",
                Some("Profile 1"),
                Some(&lad),
                &fs,
            ),
            ProfileArg::Use("Profile 1".into()),
            "an unverifiable layout must keep the user's choice, not discard it"
        );
    }

    /// The two-component derivation is what makes Chrome/Edge/Brave/Samsung
    /// work, and it must beat a coincidental one-component match.
    #[test]
    fn the_two_component_form_wins_over_the_one_component_form() {
        let fs = only(&[
            // Both exist; the vendor-qualified one is correct.
            r"C:\Users\me\AppData\Local\Chrome\User Data",
            r"C:\Users\me\AppData\Local\Google\Chrome\User Data",
        ]);
        let lad = PathBuf::from(r"C:\Users\me\AppData\Local");
        assert_eq!(
            user_data_dir_for(
                Path::new(r"C:\Program Files\Google\Chrome\Application\chrome.exe"),
                Some(&lad),
                &fs,
            ),
            Some(PathBuf::from(r"C:\Users\me\AppData\Local\Google\Chrome\User Data"))
        );
    }
}

#[cfg(test)]
mod param_and_label_tests {
    use super::*;

    #[test]
    fn params_put_the_switch_before_the_url_and_quote_both() {
        assert_eq!(
            build_launch_params(Some("Profile 1"), Some("https://example.com")).as_deref(),
            Some(r#"--profile-directory="Profile 1" "https://example.com""#),
            "Chromium wants switches before the positional URL, and 'Profile 1' \
             always contains a space"
        );
    }

    #[test]
    fn a_profile_only_launch_has_no_url() {
        assert_eq!(
            build_launch_params(Some("Profile 1"), None).as_deref(),
            Some(r#"--profile-directory="Profile 1""#)
        );
    }

    #[test]
    fn a_url_with_no_profile_is_just_the_url() {
        assert_eq!(
            build_launch_params(None, Some("https://x.dev")).as_deref(),
            Some(r#""https://x.dev""#)
        );
    }

    #[test]
    fn nothing_to_pass_is_none_not_an_empty_string() {
        // An empty lpParameters is not the same as no lpParameters for
        // ShellExecuteExW; `None` is what `shell_launch` already expects.
        assert_eq!(build_launch_params(None, None), None);
        assert_eq!(build_launch_params(Some("  "), Some("")), None);
    }

    /// The HUD must be BYTE-IDENTICAL for every binding that has no profile.
    #[test]
    fn the_hud_label_is_unchanged_without_a_profile() {
        assert_eq!(hud_label("Brave", None), "Brave");
        assert_eq!(hud_label("Brave", Some("")), "Brave");
        assert_eq!(hud_label("Brave", Some("   ")), "Brave");
    }

    #[test]
    fn the_hud_label_names_the_profile_when_there_is_one() {
        assert_eq!(hud_label("Brave", Some("Studies")), "Brave — Studies");
    }

    /// "Brave — Brave" is noise. Say it once.
    #[test]
    fn the_hud_label_does_not_repeat_itself() {
        assert_eq!(hud_label("Brave", Some("Brave")), "Brave");
        assert_eq!(hud_label("Brave", Some("brave")), "Brave");
    }
}

#[cfg(test)]
mod resolution_tests {
    use super::*;

    #[test]
    fn a_quoted_registry_command_yields_just_the_exe() {
        assert_eq!(
            exe_from_command(r#""C:\Program Files\BraveSoftware\Brave-Browser\Application\brave.exe""#)
                .as_deref(),
            Some(r"C:\Program Files\BraveSoftware\Brave-Browser\Application\brave.exe")
        );
    }

    #[test]
    fn an_unquoted_registry_command_stops_at_the_exe() {
        assert_eq!(
            exe_from_command(r"C:\Program Files\Internet Explorer\iexplore.exe --new-window")
                .as_deref(),
            Some(r"C:\Program Files\Internet Explorer\iexplore.exe")
        );
    }

    /// The install directory and the LOCALAPPDATA data directory are matched by
    /// their trailing components — the relationship that actually holds on this
    /// machine for all four registry-resolved browsers.
    #[test]
    fn install_dir_matches_its_localappdata_product_folder() {
        let cases = [
            (r"C:\Program Files\Google\Chrome", r"C:\Users\me\AppData\Local\Google\Chrome"),
            (r"C:\Program Files (x86)\Microsoft\Edge", r"C:\Users\me\AppData\Local\Microsoft\Edge"),
            (
                r"C:\Program Files\BraveSoftware\Brave-Browser",
                r"C:\Users\me\AppData\Local\BraveSoftware\Brave-Browser",
            ),
            (r"C:\Program Files\Samsung\Internet", r"C:\Users\me\AppData\Local\Samsung\Internet"),
        ];
        for (install, data) in cases {
            assert!(
                product_paths_match(Path::new(install), Path::new(data)),
                "{install} should match {data}"
            );
        }
    }

    /// Two different browsers must not be matched to each other.
    #[test]
    fn unrelated_products_do_not_match() {
        assert!(!product_paths_match(
            Path::new(r"C:\Program Files\Google\Chrome"),
            Path::new(r"C:\Users\me\AppData\Local\BraveSoftware\Brave-Browser"),
        ));
        // Same leaf name, different vendor: the two-component form separates
        // them, and the one-component fallback must not re-join them.
        assert!(!product_paths_match(
            Path::new(r"C:\Program Files\VendorA\Internet"),
            Path::new(r"C:\Users\me\AppData\Local\VendorB\Internet"),
        ));
    }

    #[test]
    fn helper_executables_are_never_chosen_as_the_browser() {
        for bad in [
            "samsunginternet_proxy", "chrome_proxy", "braveupdate", "setup",
            "crashpad_handler", "notification_helper", "elevation_service",
        ] {
            assert!(is_helper_exe(bad), "{bad} must be rejected as a launcher");
        }
        for good in ["chrome", "brave", "msedge", "samsunginternet", "arc", "helium"] {
            assert!(!is_helper_exe(good), "{good} must be accepted as a launcher");
        }
    }

    /// The noise filter must not exclude a real browser's own folders.
    #[test]
    fn the_skip_list_does_not_swallow_a_browser() {
        for keep in [
            "Google", "Chrome", "BraveSoftware", "Brave-Browser", "User Data", "Arc",
            // The Opera family's data dirs and Vivaldi's product dir must stay
            // walkable or the 2026-08-26 layout support is dead on arrival.
            "Opera Software", "Opera Stable", "Opera GX Stable", "Vivaldi",
        ] {
            assert!(!is_skipped(keep), "{keep} must remain walkable");
        }
        for drop in ["EBWebView", "Cache", "Crashpad", "Profile 1", "System Profile"] {
            assert!(is_skipped(drop), "{drop} must be skipped");
        }
    }

    #[test]
    fn product_folder_names_are_humanised() {
        assert_eq!(humanise("Brave-Browser"), "Brave Browser");
        assert_eq!(humanise("Arc"), "Arc");
    }
}

/// `profiles_from_local_state`'s email extraction (PROBLEM 223), against real
/// files on disk rather than a hand-built `serde_json::Value` — the shape that
/// actually matters is what a `Local State` file looks like once written and
/// re-read, and that is what `scan_browsers` will hand this function too.
///
/// Fixture shapes are the MEASURED ones from `BrowserProfile::email`'s doc
/// comment: Chrome-shaped (signed in, real email + a `gaia_name` that is a
/// person's name, not an email), Edge/Samsung-shaped (`user_name: ""`,
/// present but empty), and Brave-shaped (`user_name: ""`, `gaia_name` key
/// absent entirely). One test also covers a value this app has not measured
/// anywhere but which the same `filter` must still handle correctly:
/// whitespace-only.
#[cfg(test)]
mod local_state_email_tests {
    use super::*;
    use std::io::Write;

    /// A unique scratch dir per test (PROBLEM 130's rule: tests run on
    /// parallel threads, so nothing may share a path).
    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("spaceadom-bp-email-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// Write `<dir>/Local State` with the given raw `info_cache` JSON object
    /// body, and `<dir>/<profile dir>` so the profile survives the "folder
    /// still exists" filter every candidate must pass.
    fn write_local_state(dir: &Path, profile_dir: &str, info_cache_entry: &str) {
        std::fs::create_dir_all(dir.join(profile_dir)).unwrap();
        let body = format!(
            r#"{{"profile":{{"info_cache":{{"{profile_dir}":{info_cache_entry}}}}}}}"#
        );
        let mut f = std::fs::File::create(dir.join("Local State")).unwrap();
        f.write_all(body.as_bytes()).unwrap();
        f.sync_all().unwrap();
    }

    /// Chrome-shaped: signed in. `user_name` is a real email; `gaia_name` is
    /// populated too, but with a PERSON'S NAME — proof the extraction reads
    /// `user_name` and not `gaia_name`, since the two disagree in this
    /// fixture and only one of them is email-shaped.
    #[test]
    fn a_signed_in_chrome_profile_yields_its_user_name_as_email() {
        let d = scratch("chrome-signed-in");
        write_local_state(
            &d,
            "Profile 1",
            r#"{"name":"Work","user_name":"test.user@example.com","gaia_name":"Test Person"}"#,
        );
        let profiles = profiles_from_local_state(&d).expect("valid Chromium shape");
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].email.as_deref(), Some("test.user@example.com"));
        assert_ne!(
            profiles[0].email.as_deref(),
            Some("Test Person"),
            "gaia_name must never leak into the email field — it is a display \
             name, not an address"
        );
    }

    /// Edge/Samsung-shaped: `user_name` is PRESENT but an empty string — not
    /// absent from the JSON at all. Measured on both real browsers on the
    /// owner's machine. Must read as no email, not as an empty-string email.
    #[test]
    fn an_empty_string_user_name_is_not_signed_in() {
        let d = scratch("edge-empty-string");
        write_local_state(
            &d,
            "Profile 1",
            r#"{"name":"Profile 1","user_name":"","gaia_name":""}"#,
        );
        let profiles = profiles_from_local_state(&d).expect("valid Chromium shape");
        assert_eq!(profiles[0].email, None);
    }

    /// Brave-shaped: `user_name` is `""` and `gaia_name` is ABSENT from the
    /// JSON entirely (not even `null`) — a different cause of the same
    /// symptom, measured on the owner's real Brave install. The extraction
    /// must not care either way: no `user_name` key or an empty one must both
    /// collapse to `None`.
    #[test]
    fn a_missing_user_name_key_is_also_not_signed_in() {
        let d = scratch("brave-no-key");
        // No `user_name` key at all — only `name`, matching Brave's shape for
        // a profile that has never seen a sign-in prompt.
        write_local_state(&d, "Default", r#"{"name":"Default"}"#);
        let profiles = profiles_from_local_state(&d).expect("valid Chromium shape");
        assert_eq!(profiles[0].email, None);
    }

    /// A value this app has never actually measured, but the same `filter`
    /// that strips "" must also strip whitespace-only — the same rule
    /// `display_name`'s extraction already applies just above this code.
    #[test]
    fn a_whitespace_only_user_name_is_not_signed_in() {
        let d = scratch("whitespace-only");
        write_local_state(&d, "Default", r#"{"name":"Default","user_name":"   "}"#);
        let profiles = profiles_from_local_state(&d).expect("valid Chromium shape");
        assert_eq!(profiles[0].email, None);
    }

    /// A real email is trimmed of surrounding whitespace, same as the display
    /// name is — Chromium has never been observed to pad the field, but
    /// nothing here should depend on that staying true.
    #[test]
    fn a_padded_user_name_is_trimmed() {
        let d = scratch("padded");
        write_local_state(
            &d,
            "Default",
            r#"{"name":"Default","user_name":"  padded@example.com  "}"#,
        );
        let profiles = profiles_from_local_state(&d).expect("valid Chromium shape");
        assert_eq!(profiles[0].email.as_deref(), Some("padded@example.com"));
    }

    /// Two profiles in one `Local State`, one signed in and one not — the
    /// ordinary mixed case on a real machine, and proof `None` for one entry
    /// cannot bleed into `Some` for its neighbour or vice versa.
    #[test]
    fn signed_in_and_signed_out_profiles_coexist_in_one_file() {
        let d = scratch("mixed");
        std::fs::create_dir_all(d.join("Default")).unwrap();
        std::fs::create_dir_all(d.join("Profile 1")).unwrap();
        let body = r#"{"profile":{"info_cache":{
            "Default":{"name":"Primary","user_name":"primary.user@example.com"},
            "Profile 1":{"name":"Guest","user_name":""}
        }}}"#;
        let mut f = std::fs::File::create(d.join("Local State")).unwrap();
        f.write_all(body.as_bytes()).unwrap();
        f.sync_all().unwrap();

        let profiles = profiles_from_local_state(&d).expect("valid Chromium shape");
        let default = profiles.iter().find(|p| p.directory == "Default").unwrap();
        let guest = profiles.iter().find(|p| p.directory == "Profile 1").unwrap();
        assert_eq!(default.email.as_deref(), Some("primary.user@example.com"));
        assert_eq!(guest.email, None);
        // …and the LABEL each one renders with follows the same split.
        assert_eq!(default.account_label, "primary.user");
        assert_eq!(guest.account_label, "Guest");
    }

    /// The label is derived at scan time, not at render time, so it is pinned
    /// on the real parse path and not only on the pure helper.
    #[test]
    fn a_signed_in_profile_is_labelled_by_its_email_local_part() {
        let d = scratch("label");
        std::fs::create_dir_all(d.join("Default")).unwrap();
        let body = r#"{"profile":{"info_cache":{
            "Default":{"name":"Person 3","user_name":"test.user@example.com","gaia_name":"Test User"}
        }}}"#;
        let mut f = std::fs::File::create(d.join("Local State")).unwrap();
        f.write_all(body.as_bytes()).unwrap();
        f.sync_all().unwrap();

        let profiles = profiles_from_local_state(&d).expect("valid Chromium shape");
        assert_eq!(profiles[0].account_label, "test.user");
        assert_eq!(
            profiles[0].display_name, "Person 3",
            "the browser's own name must still be carried — it is the tile's second line"
        );
        assert!(
            !profiles[0].account_label.contains('@'),
            "the label is the LOCAL PART; a full address must never be the headline"
        );
    }
}

/// The account label — the owner's 2026-08-31 reversal of the shipped design.
///
/// Pure, so it is tested without touching the filesystem. The cases that
/// matter are the malformed ones: `user_name` is free text in someone else's
/// JSON, and every value that is not address-shaped must fall back to the
/// display name rather than render half an address.
#[cfg(test)]
mod account_label_tests {
    use super::{account_label, email_local_part};

    #[test]
    fn a_real_address_yields_everything_before_the_at() {
        assert_eq!(email_local_part("test.user@example.com").as_deref(), Some("test.user"));
        assert_eq!(email_local_part("  padded@example.com  ").as_deref(), Some("padded"));
        // A `+` tag is part of the local part and stays — it is how the user
        // distinguishes two accounts on one domain, which is the whole point.
        assert_eq!(email_local_part("me+work@example.com").as_deref(), Some("me+work"));
    }

    #[test]
    fn anything_not_address_shaped_is_refused() {
        assert_eq!(email_local_part(""), None);
        assert_eq!(email_local_part("   "), None);
        assert_eq!(email_local_part("Nur Arpon"), None, "a display name is not an address");
        assert_eq!(email_local_part("@example.com"), None, "no local part");
        assert_eq!(email_local_part("me@"), None, "no domain");
        assert_eq!(email_local_part("  @  "), None);
        assert_eq!(email_local_part("a@b@c.com"), None, "two @ — refuse, do not guess");
    }

    #[test]
    fn the_label_falls_back_to_the_display_name_when_not_signed_in() {
        assert_eq!(account_label("ARPON'S STUDIES", None), "ARPON'S STUDIES");
        assert_eq!(account_label("Default", Some("")), "Default");
        assert_eq!(account_label("Person 3", Some("not-an-address")), "Person 3");
    }

    #[test]
    fn the_label_prefers_the_email_when_there_is_one() {
        assert_eq!(account_label("Person 3", Some("nur.arpon@example.com")), "nur.arpon");
    }

    /// The reason this feature exists: two profiles whose display names are
    /// indistinguishable become distinguishable.
    #[test]
    fn two_identically_named_profiles_get_two_different_labels() {
        let a = account_label("Person 1", Some("study.account@example.com"));
        let b = account_label("Person 1", Some("work.account@example.com"));
        assert_ne!(a, b);
    }

    /// The HUD path composes the two: the stored `browser_profile_name` is the
    /// label, and `hud_label` pairs it with the browser. Pinned together
    /// because that pairing is what the user reads on a Space-hold.
    #[test]
    fn the_label_is_what_the_hud_pairs_with_the_browser_name() {
        let label = account_label("Person 3", Some("nur.arpon@example.com"));
        assert_eq!(super::hud_label("Chrome", Some(&label)), "Chrome — nur.arpon");
    }
}

/// The Opera-style layout (2026-08-26). REASONED, NOT MEASURED: Opera and
/// Vivaldi are not installed on this machine, so the paths in these tests come
/// from their public documentation (Opera 114+), not from a live install — the
/// tests pin the DECISIONS made from that documentation, they cannot prove the
/// documentation right.
#[cfg(test)]
mod opera_layout_tests {
    use super::*;

    /// Chrome-shaped data dirs keep their old product derivation; a data dir
    /// whose `Local State` is not inside `User Data` IS its own product folder.
    #[test]
    fn the_product_folder_depends_on_the_layout() {
        assert_eq!(
            product_dir_for(Path::new(r"C:\Users\me\AppData\Local\Google\Chrome\User Data")),
            Some((
                PathBuf::from(r"C:\Users\me\AppData\Local\Google\Chrome"),
                DataLayout::NestedUserData
            )),
        );
        assert_eq!(
            product_dir_for(Path::new(r"C:\Users\me\AppData\Roaming\Opera Software\Opera Stable")),
            Some((
                PathBuf::from(r"C:\Users\me\AppData\Roaming\Opera Software\Opera Stable"),
                DataLayout::SelfContained
            )),
            "an Opera-shaped dir must be its OWN product folder — deriving the \
             parent yields the vendor folder (Opera Software), which no \
             resolution source can ever match"
        );
    }

    /// The marriage that makes Opera detectable at all: install folder
    /// "Opera"/"Opera GX" against data folder "Opera Stable"/"Opera GX Stable",
    /// per-user and per-machine installs both.
    #[test]
    fn opera_install_dirs_marry_opera_data_dirs() {
        let cases = [
            (r"C:\Users\me\AppData\Local\Programs\Opera",
             r"C:\Users\me\AppData\Roaming\Opera Software\Opera Stable"),
            (r"C:\Program Files\Opera",
             r"C:\Users\me\AppData\Roaming\Opera Software\Opera Stable"),
            (r"C:\Users\me\AppData\Local\Programs\Opera GX",
             r"C:\Users\me\AppData\Roaming\Opera Software\Opera GX Stable"),
        ];
        for (install, data) in cases {
            assert!(
                product_paths_match(Path::new(install), Path::new(data)),
                "{install} should marry {data}"
            );
        }
    }

    /// Opera and Opera GX are DIFFERENT browsers sharing a vendor folder and an
    /// exe name — the suffix tolerance must not join them.
    #[test]
    fn opera_and_opera_gx_do_not_marry_each_other() {
        assert!(!product_paths_match(
            Path::new(r"C:\Users\me\AppData\Local\Programs\Opera"),
            Path::new(r"C:\Users\me\AppData\Roaming\Opera Software\Opera GX Stable"),
        ));
        assert!(!product_paths_match(
            Path::new(r"C:\Users\me\AppData\Local\Programs\Opera GX"),
            Path::new(r"C:\Users\me\AppData\Roaming\Opera Software\Opera Stable"),
        ));
    }

    /// The claim "Vivaldi needs nothing" rests on this: a per-machine install
    /// (`C:\Program Files\Vivaldi`) marries the `%LOCALAPPDATA%\Vivaldi` data
    /// product through the one-component leaf fallback that has been there
    /// since the module was written. (The per-user install needs no marriage at
    /// all — its exe is under `<product>\Application`, source 1.)
    #[test]
    fn a_per_machine_vivaldi_marries_its_data_dir() {
        assert!(product_paths_match(
            Path::new(r"C:\Program Files\Vivaldi"),
            Path::new(r"C:\Users\me\AppData\Local\Vivaldi"),
        ));
    }

    /// The username must not be treated as a vendor. Before 2026-08-26,
    /// `…\AppData\Local\Programs\Opera` read as ["users","me","opera"], the
    /// 2-component comparison fired, and "me" was compared against
    /// "opera software" — so every unnested per-user install failed to marry.
    #[test]
    fn the_username_is_not_an_identity_component() {
        // Same product, one side per-user: must still match.
        assert!(product_paths_match(
            Path::new(r"C:\Users\me\AppData\Local\Vivaldi"),
            Path::new(r"C:\Program Files\Vivaldi"),
        ));
        // The Samsung guard from the original tests must survive the change:
        // two REAL vendor components still have to agree.
        assert!(!product_paths_match(
            Path::new(r"C:\Users\me\AppData\Local\VendorB\Internet"),
            Path::new(r"C:\Program Files\VendorA\Internet"),
        ));
    }

    /// The plausibility gate for the skip log: at least one live profile, and
    /// not under a known embedded-webview dir. EBWebView never reaches the scan
    /// today (SKIP_DIRS prunes it) — this pins the belt-and-braces behaviour if
    /// that ever changes.
    #[test]
    fn plausibility_is_conservative() {
        let profiles = vec![BrowserProfile {
            directory: "Default".into(),
            display_name: "Default".into(),
            email: None,
            account_label: "Default".into(),
        }];
        assert!(looks_like_plausible_browser(
            Path::new(r"C:\Users\me\AppData\Local\Spotify"),
            &profiles
        ));
        assert!(!looks_like_plausible_browser(
            Path::new(r"C:\Users\me\AppData\Roaming\App\EBWebView"),
            &profiles
        ));
        assert!(!looks_like_plausible_browser(
            Path::new(r"C:\Users\me\AppData\Local\Spotify"),
            &[]
        ));
    }

    /// The suffix rule is " stable" only, at the end only.
    #[test]
    fn the_channel_suffix_rule_is_narrow() {
        assert_eq!(strip_channel_suffix("opera stable"), "opera");
        assert_eq!(strip_channel_suffix("opera gx stable"), "opera gx");
        assert_eq!(strip_channel_suffix("stable opera"), "stable opera");
        assert_eq!(strip_channel_suffix("chrome beta"), "chrome beta");
        // "unstable" must not lose its tail through a sloppy suffix match.
        assert_eq!(strip_channel_suffix("unstable"), "unstable");
    }
}

/// A LIVE look at this machine. Ignored by default — it walks AppData for a few
/// seconds and its result depends on what is installed, so it is a diagnostic,
/// not a pass/fail gate.
///
/// Run: `cargo test --lib -- --ignored --nocapture live_browser_scan`
#[cfg(all(test, windows))]
mod live_scan {
    #[test]
    #[ignore]
    fn live_browser_scan() {
        let started = std::time::Instant::now();
        let found = super::scan_browsers();
        println!("scanned in {}ms; {} browser(s)", started.elapsed().as_millis(), found.len());
        for b in &found {
            println!("\n  {} -> {}", b.browser_name, b.browser_exe);
            println!("    user data: {}", b.user_data_dir);
            for p in &b.profiles {
                // The LABEL, never the address. A full email must not reach
                // any output stream — this diagnostic prints exactly what the
                // UI shows, which is also all it needs to be useful.
                println!(
                    "      [{}] {} (name: {}, signed in: {})",
                    p.directory,
                    p.account_label,
                    p.display_name,
                    if p.email.is_some() { "yes" } else { "no" }
                );
            }
        }
        assert!(!found.is_empty(), "no Chromium browser found on this machine");
    }
}

/// The window -> profile matching side (2026-08-27).
///
/// Every one of these is a branch reached only after something has already
/// gone wrong or gone ambiguous, which is exactly the class CLAUDE.md says to
/// test: a wrong answer here does not crash, it silently minimises the wrong
/// window - the failure the owner actually reported.
#[cfg(test)]
mod window_profile_tests {
    use super::*;
    use crate::config::KeyBinding;

    // The two RelaunchCommand strings measured on this machine, verbatim.
    const BRAVE_RC: &str = r#""C:\Program Files\BraveSoftware\Brave-Browser\Application\brave.exe" --profile-directory="Profile 1""#;
    const CHROME_RC: &str = r#""C:\Program Files\Google\Chrome\Application\chrome.exe" --profile-directory="Profile 6""#;

    #[test]
    fn the_measured_relaunch_commands_parse_to_their_folder_names() {
        assert_eq!(
            profile_dir_from_relaunch_command(BRAVE_RC).as_deref(),
            Some("Profile 1"),
            "the space is part of the folder name and must survive the parse"
        );
        assert_eq!(
            profile_dir_from_relaunch_command(CHROME_RC).as_deref(),
            Some("Profile 6")
        );
    }

    /// THE DEFAULT-PROFILE SHAPE, verbatim from a live Edge window measured by
    /// `live_profile_probe` on 2026-08-27. Note the value is UNQUOTED, and note
    /// that the AUMID is BARE - no `.UserData.` segment at all, so the relaunch
    /// command is the only thing identifying this window. Get either of these
    /// wrong and every Edge binding silently stops toggling.
    #[test]
    fn the_measured_default_profile_window_parses() {
        const EDGE_RC: &str =
            r#""C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe" --profile-directory=Default"#;
        assert_eq!(
            profile_dir_from_relaunch_command(EDGE_RC).as_deref(),
            Some("Default")
        );
        assert_eq!(profile_token_from_aumid("MSEdge"), None);
        // ...and the two together must still identify it: a bare AUMID is not
        // a contradiction, it is simply silence.
        assert!(same_profile_dir("Default", "default"));
    }

    /// A relaunch command with no switch at all has still never been observed.
    /// It must come back as `None` - "unproven", which the caller turns into
    /// "launch, do not minimise" - rather than being read as Default.
    #[test]
    fn a_command_line_with_no_profile_flag_proves_nothing() {
        let no_flag = r#""C:\Program Files\BraveSoftware\Brave-Browser\Application\brave.exe""#;
        assert_eq!(profile_dir_from_relaunch_command(no_flag), None);
        // The quoted spelling of Default, for the browsers that use it.
        let explicit = r#""C:\...\brave.exe" --profile-directory="Default""#;
        assert_eq!(
            profile_dir_from_relaunch_command(explicit).as_deref(),
            Some("Default")
        );
    }

    #[test]
    fn an_empty_or_absent_profile_value_is_not_a_profile() {
        assert_eq!(
            profile_dir_from_relaunch_command(r#""x.exe" --profile-directory="""#),
            None
        );
        assert_eq!(profile_dir_from_relaunch_command(""), None);
        assert_eq!(profile_dir_from_relaunch_command("--profile-directory="), None);
    }

    /// Chromium quotes anything with a space, so an unquoted value is a single
    /// token. Parse it anyway rather than returning None: a browser that omits
    /// the quotes is unproven, not wrong.
    #[test]
    fn an_unquoted_value_is_read_as_one_token() {
        assert_eq!(
            profile_dir_from_relaunch_command(r"C:\x\brave.exe --profile-directory=Default --foo")
                .as_deref(),
            Some("Default")
        );
    }

    /// The flag name is matched case-insensitively, and the match must be made
    /// against an ASCII-lowered copy so a non-ASCII install path cannot shift
    /// the byte offsets of everything after it.
    #[test]
    fn a_non_ascii_install_path_does_not_shift_the_parse() {
        let cmd = r#""C:\Programme\Bücher\Straße\brave.exe" --PROFILE-DIRECTORY="Profile 3""#;
        assert_eq!(
            profile_dir_from_relaunch_command(cmd).as_deref(),
            Some("Profile 3")
        );
    }

    #[test]
    fn the_measured_aumids_yield_their_profile_token() {
        assert_eq!(
            profile_token_from_aumid("Brave.UserData.Profile1").as_deref(),
            Some("Profile1")
        );
        assert_eq!(
            profile_token_from_aumid("Chrome.UserData.Profile6").as_deref(),
            Some("Profile6")
        );
    }

    #[test]
    fn an_aumid_with_no_profile_segment_yields_nothing() {
        // The bare form, and a Store AUMID, and junk. All unproven.
        assert_eq!(profile_token_from_aumid("Brave.UserData"), None);
        assert_eq!(profile_token_from_aumid("Brave.UserData."), None);
        assert_eq!(
            profile_token_from_aumid("SamsungElectronicsCo.Ltd.PCGallery_3c1yjt4zspk6g!App"),
            None
        );
        assert_eq!(profile_token_from_aumid(""), None);
    }

    /// THE TRAP. RelaunchCommand says `Profile 1`; the AUMID for the SAME
    /// window says `Profile1`. A matcher that compares them verbatim matches
    /// nothing, forever, silently. If this test ever fails, the profile feature
    /// is dead and nothing else will say so.
    #[test]
    fn the_aumid_space_stripping_is_normalised_away() {
        assert!(same_profile_dir("Profile 1", "Profile1"));
        assert!(same_profile_dir("Profile1", "Profile 1"));
        assert!(same_profile_dir("profile 1", "Profile 1"));
        assert!(same_profile_dir("Default", "default"));
    }

    #[test]
    fn different_profiles_stay_different() {
        assert!(!same_profile_dir("Profile 1", "Profile 11"));
        assert!(!same_profile_dir("Profile 1", "Profile 2"));
        assert!(!same_profile_dir("Default", "Profile 1"));
    }

    /// "We read nothing" must never compare equal to "we read nothing".
    /// Two unreadable windows are not the same window.
    #[test]
    fn an_empty_name_matches_nothing_including_another_empty_one() {
        assert!(!same_profile_dir("", ""));
        assert!(!same_profile_dir("", "Profile 1"));
        assert!(!same_profile_dir("Profile 1", ""));
        assert!(!same_profile_dir("   ", "Profile 1"));
    }

    fn app_binding(app: &str, dir: Option<&str>) -> KeyBinding {
        KeyBinding {
            app: Some(app.to_string()),
            browser_profile_dir: dir.map(str::to_string),
            ..Default::default()
        }
    }

    fn url_binding(url: &str, exe: Option<&str>, dir: Option<&str>) -> KeyBinding {
        KeyBinding {
            web_url: Some(url.to_string()),
            browser_exe: exe.map(str::to_string),
            browser_profile_dir: dir.map(str::to_string),
            ..Default::default()
        }
    }

    /// The owner's scenario exactly: Space+B unpinned Brave, Space+N pinned to
    /// Brave "Profile 1". Only N contributes a claim.
    #[test]
    fn only_pinned_bindings_claim_a_profile() {
        let b = app_binding(r"C:\Program Files\BraveSoftware\Brave-Browser\Application\brave.exe", None);
        let n = app_binding(
            r"C:\Program Files\BraveSoftware\Brave-Browser\Application\brave.exe",
            Some("Profile 1"),
        );
        let claims = profile_claims([&b, &n]);
        assert_eq!(claims, vec![("brave".to_string(), "Profile 1".to_string())]);
    }

    /// A claim is per BROWSER, not global. Chrome's "Profile 1" must not make
    /// an unpinned BRAVE binding decline Brave's "Profile 1" window.
    #[test]
    fn a_claim_belongs_to_one_browser_only() {
        let chrome = url_binding(
            "https://mail.google.com",
            Some(r"C:\Program Files\Google\Chrome\Application\chrome.exe"),
            Some("Profile 1"),
        );
        let claims = profile_claims([&chrome]);
        assert_eq!(claims, vec![("chrome".to_string(), "Profile 1".to_string())]);
        assert!(!claims.iter().any(|(stem, _)| stem == "brave"));
    }

    /// A URL binding names its browser in `browser_exe`; an app binding's `app`
    /// IS the browser. Same precedence the launch leg uses.
    #[test]
    fn browser_exe_wins_over_app_when_both_are_present() {
        let mixed = KeyBinding {
            app: Some(r"C:\weird\brave.exe".into()),
            web_url: Some("https://example.com".into()),
            browser_exe: Some(r"C:\Program Files\Google\Chrome\Application\chrome.exe".into()),
            browser_profile_dir: Some("Profile 6".into()),
            ..Default::default()
        };
        assert_eq!(
            profile_claims([&mixed]),
            vec![("chrome".to_string(), "Profile 6".to_string())]
        );
    }

    /// A frontend that writes `""` instead of `null` is an ordinary bug; the
    /// correct response is "no claim", not a claim on an empty folder name that
    /// would then match nothing and quietly disable the whole rule.
    #[test]
    fn blank_and_missing_fields_claim_nothing() {
        let blank_dir = app_binding(r"C:\x\brave.exe", Some("   "));
        let no_dir = app_binding(r"C:\x\brave.exe", None);
        let no_exe = KeyBinding {
            browser_profile_dir: Some("Profile 1".into()),
            ..Default::default()
        };
        assert!(profile_claims([&blank_dir, &no_dir, &no_exe]).is_empty());
    }

    /// The same profile pinned on two keys is one claim, not two. Nothing
    /// depends on the count, but an unbounded list on a latency path is the
    /// kind of thing that grows teeth later.
    #[test]
    fn the_same_pin_on_two_keys_is_one_claim() {
        let a = app_binding(r"C:\x\brave.exe", Some("Profile 1"));
        // Spelled the AUMID way on purpose - it is still the same profile.
        let b = app_binding(r"C:\x\brave.exe", Some("Profile1"));
        assert_eq!(profile_claims([&a, &b]).len(), 1);
    }

    /// Claims come from the ACTIVE profile plus the (global) special keys, and
    /// from nowhere else - a pin sitting in a profile the user is not currently
    /// in must not make their active bindings decline windows.
    #[test]
    fn claims_come_from_the_active_profile_and_the_special_keys() {
        use crate::config::{AppConfig, Profile};
        use crate::config::BindingMap;

        let mut active = BindingMap::new();
        active.insert(
            "n".to_string(),
            app_binding(r"C:\x\brave.exe", Some("Profile 1")),
        );
        let mut idle = BindingMap::new();
        idle.insert(
            "q".to_string(),
            app_binding(r"C:\x\brave.exe", Some("Profile 9")),
        );
        let mut specials = BindingMap::new();
        specials.insert(
            "F5".to_string(),
            url_binding("https://x.example", Some(r"C:\x\chrome.exe"), Some("Profile 6")),
        );

        let cfg = AppConfig {
            active_profile: "Work".to_string(),
            profiles: vec![
                Profile { name: "Work".into(), bindings: active, emoji: None },
                Profile { name: "Games".into(), bindings: idle, emoji: None },
            ],
            special_keys: specials,
            ..Default::default()
        };

        let claims = active_profile_claims(&cfg);
        assert!(claims.contains(&("brave".to_string(), "Profile 1".to_string())));
        assert!(claims.contains(&("chrome".to_string(), "Profile 6".to_string())));
        assert!(
            !claims.iter().any(|(_, d)| d == "Profile 9"),
            "an inactive profile's pin must not claim anything"
        );
        assert_eq!(claims.len(), 2);
    }
}
