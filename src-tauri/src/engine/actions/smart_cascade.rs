/// engine/actions/smart_cascade.rs — Port of V11 SmartCascade() + ResolvePath()
// Trivial update to refresh IDE diagnostics.

use crate::config::KeyBinding;

#[cfg(windows)]
use windows::Win32::{
    Foundation::HWND,
    System::Threading::AttachThreadInput,
    UI::WindowsAndMessaging::{
        EnumWindows, GetWindowLongW,
        IsWindowVisible, ShowWindow, SetForegroundWindow, GetForegroundWindow,
        IsWindow, GetWindowThreadProcessId, IsIconic, IsHungAppWindow,
        BringWindowToTop, SwitchToThisWindow,
        GWL_STYLE, SW_MINIMIZE, SW_RESTORE, WS_VISIBLE,
    },
    UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_TYPE, KEYBDINPUT, KEYEVENTF_KEYUP, KEYBD_EVENT_FLAGS,
        // VK_MENU was dropped in PROBLEM 225 — see `force_foreground` step 3
        // for why a bare Alt tap is not a benign key to inject.
        SendInput, VIRTUAL_KEY,
    },
};
use windows::Win32::System::Threading::GetCurrentThreadId;

use std::sync::{Arc, Mutex, OnceLock};
use std::collections::HashMap;

/// The cascade's HWND cache, keyed by BINDING IDENTITY (see `cache_key`).
///
/// It used to be keyed by the exe STEM alone, and that is what actually fired
/// the wrong minimise in the owner's log on 2026-08-26: two bindings on
/// brave.exe were one string, one cache entry and one HWND. A perfect window
/// matcher would still have been defeated by it, because the cache-hit branch
/// never re-enumerates. Process-wide and never cleared wholesale - individual
/// entries are evicted when they fail re-validation.
#[cfg(windows)]
fn get_app_cache() -> Arc<Mutex<HashMap<String, isize>>> {
    static CACHE: OnceLock<Arc<Mutex<HashMap<String, isize>>>> = OnceLock::new();
    CACHE.get_or_init(|| Arc::new(Mutex::new(HashMap::new()))).clone()
}

// ---------------------------------------------------------------------------
// BINDING IDENTITY (2026-08-27) - which window is allowed to be "this key's"
// ---------------------------------------------------------------------------
//
// See the long measurement note at the top of `browser_profiles.rs` for what
// was tried and what actually distinguishes two profiles of one browser. This
// half is the decision: given a binding on one side and a window on the other,
// may we touch it?
//
// THE ONE RULE THAT MATTERS MOST, and it is the reason NATIVE_SAFETY.md exists:
// when we cannot PROVE a window belongs to this binding, we do not match it.
// Falling through to `launch_binding_app` launches with `--profile-directory=`
// and Chromium itself turns that into a focus of the correct window, so the
// cost of being wrong in that direction is nil. The cost of being wrong in the
// other direction is minimising a window the user did not aim at - which is the
// class of bug that once broke this owner's touchpad.

/// What a binding will accept as "its" window.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ProfileRule {
    /// No profile discrimination at all.
    ///
    /// EVERY binding that existed before browser profiles, every non-browser
    /// binding, and every browser binding on a machine where nothing else pins
    /// a profile of that browser lands here - and on this arm the matcher does
    /// exactly what it did before this change, down to reading no window
    /// properties and initialising no COM. That is the property that makes
    /// this change safe to ship: the new code is not on the path at all unless
    /// the user has actually pinned something.
    Any,
    /// This binding pins a Chromium profile folder. Only a window that PROVES
    /// it belongs to that profile may be touched.
    Pinned(String),
    /// This binding pins nothing, but other bindings the user can reach right
    /// now pin profiles of THIS browser. Windows that prove they belong to one
    /// of those are left alone; everything else is fair game.
    ///
    /// Owner's decision, 2026-08-27, not to be re-opened: *an unpinned browser
    /// binding matches any window of that browser EXCEPT one whose profile is
    /// claimed by another binding's pin in the active profile.* So Space+B
    /// takes whatever Brave window is not Space+N's, rather than stealing it,
    /// and if every window is claimed it falls through to launch.
    Unpinned(Vec<String>),
}

/// What a WINDOW says about itself.
#[derive(Debug, Clone, PartialEq, Eq)]
enum WindowProfile {
    /// A profile folder was read off this window. The ONLY variant that can
    /// ever authorise a minimise.
    Named(String),
    /// The window carries a Chromium relaunch command that contains no
    /// `--profile-directory` switch at all.
    ///
    /// STILL NEVER OBSERVED, and that is the point of the variant. The
    /// default-profile case turned out NOT to look like this: a live Edge
    /// window measured 2026-08-27 (see `live_profile_probe`, and the table at
    /// the top of `browser_profiles.rs`) carries
    /// `--profile-directory=Default`, unquoted, and comes back as
    /// `Named("Default")` like any other profile. So this variant is what the
    /// code does instead of ASSUMING a shape for a window nobody has seen —
    /// and it is produced only when the switch is genuinely absent, because a
    /// switch that is present but unparseable produces `Unknown`. A parser miss
    /// can never be mistaken for a default-profile window.
    NoProfileNamed,
    /// Nothing usable: no property store, no relaunch command, an unreadable
    /// `--profile-directory`, or two properties that contradict each other.
    Unknown,
}

/// Does a window carrying this evidence belong to a binding with this rule?
///
/// Pure, and tested, because every branch here is one a user only reaches after
/// something has already gone ambiguous - and a wrong answer does not crash, it
/// silently minimises the wrong window.
fn evidence_satisfies(evidence: &WindowProfile, rule: &ProfileRule) -> bool {
    use crate::browser_profiles::same_profile_dir;
    match rule {
        ProfileRule::Any => true,

        // FAIL SAFE. Missing, unreadable, or a different profile - all three
        // mean "not proven mine", and all three fall through to launch.
        ProfileRule::Pinned(want) => match evidence {
            WindowProfile::Named(got) => same_profile_dir(got, want),
            WindowProfile::NoProfileNamed | WindowProfile::Unknown => false,
        },

        ProfileRule::Unpinned(claimed) => match evidence {
            // The owner's rule, literally: anything not claimed by someone else.
            WindowProfile::Named(got) => {
                !claimed.iter().any(|c| same_profile_dir(got, c))
            }
            // A window whose relaunch command names NO profile cannot be one
            // of the explicitly named claims - Chromium would be writing a
            // relaunch command that reopens the wrong profile, which would
            // break Windows' own taskbar pinning. The one claim it could
            // collide with is a claim on `Default` itself, so that case
            // declines.
            //
            // THIS IS THE ONE REASONED STEP IN THE MATCHER rather than a
            // measured one, and it is deliberately the least load-bearing:
            // every browser window measured so far, default profile included,
            // names its profile explicitly and arrives as `Named`, so this arm
            // has never actually been taken. It exists so that a browser which
            // does omit the switch degrades to "toggles normally" instead of
            // "never toggles".
            WindowProfile::NoProfileNamed => {
                !claimed.iter().any(|c| same_profile_dir(c, "Default"))
            }
            // We are only here because the user HAS pinned a profile of this
            // browser somewhere. An unidentifiable window in that world is
            // exactly the ambiguity the fail-safe is for. Cost of being wrong:
            // this key focuses by relaunching instead of toggling. Cost of the
            // other choice: the reported bug, back again.
            WindowProfile::Unknown => false,
        },
    }
}

/// The cache key for a binding: the exe stem AND the profile it pins.
///
/// ONE function, called from the read site and the write site both, so the two
/// cannot drift apart - which is the failure mode that would put the original
/// bug straight back with no visible cause.
///
/// `|` cannot appear in a Windows file name, so `brave|*` (unpinned) can never
/// collide with `brave|profile 1` (pinned) or with any exe stem.
fn cache_key(exe_stem: &str, rule: &ProfileRule) -> String {
    match rule {
        // Normalised through the SAME function the window comparison uses, so
        // "Profile 1" and "Profile1" are one cache entry, not two pointing at
        // one window.
        ProfileRule::Pinned(dir) => format!(
            "{exe_stem}|{}",
            crate::browser_profiles::normalise_profile_dir(dir)
        ),
        // An UNPINNED binding shares one entry per browser, which is what it
        // did before this change - two unpinned keys on the same browser have
        // always toggled the same window, and the owner did not ask for that
        // to change. The explicit `*` marker is there so a key can never be
        // read as "a stem with no profile information attached".
        ProfileRule::Any | ProfileRule::Unpinned(_) => format!("{exe_stem}|*"),
    }
}

/// The claims-only half of the rule: what an UNPINNED binding on this browser
/// must avoid.
fn claims_rule(exe_stem: &str, claims: &[(String, String)]) -> ProfileRule {
    let mine: Vec<String> = claims
        .iter()
        .filter(|(stem, _)| stem == exe_stem)
        .map(|(_, dir)| dir.clone())
        .collect();
    if mine.is_empty() {
        ProfileRule::Any
    } else {
        ProfileRule::Unpinned(mine)
    }
}

/// The rule an APP binding matches windows of its executable by.
///
/// `exe` is the LAUNCH TARGET (a path or a bare exe name), not a stem, and that
/// is load-bearing: the answer depends on whether the pinned profile FOLDER
/// still exists, and only the exe path leads to the `User Data` directory that
/// question is asked in.
///
/// WHY IT ASKS AT ALL (review finding, 2026-08-27 — real, and fixed here).
/// This used to read `browser_profile_dir` raw and hand back `Pinned(dir)`
/// whatever the filesystem said, while the launch leg ran the same value
/// through `profile_arg_for` and DROPPED it when the folder was gone. A user
/// who deletes a pinned profile from inside the browser therefore got a key
/// that could never match again: press 1 launched Brave without the switch
/// (correctly) and pressed 2, 3, 4 … launched ANOTHER one, plus a toast each
/// time, because the match leg was still demanding a profile no living window
/// could ever have. That is CORE_AIM's "Smart Cascade" and "Cyclic Reliability"
/// broken permanently for that binding, and the file's own stated design —
/// *"the launch leg and the match leg agree about the profile in every case,
/// including the failure cases"* — was not true of the failure cases.
///
/// So the single decision function both legs already had is now asked by both
/// legs. A stale pin degrades the binding to exactly what it is once the folder
/// is gone: an UNPINNED binding on that browser, which still declines windows
/// another binding has claimed.
///
/// COST, because this is the Space-hold dispatch path: `profile_arg_for` is a
/// no-op returning `None` unless `browser_profile_dir` is set, so every binding
/// that predates this feature pays one `Option` test. A binding that DOES pin a
/// profile pays at most four `symlink_metadata` calls, against an
/// `EnumWindows` over every top-level window plus a COM property-store read per
/// candidate in the very same press. Measured shape, not a guess: the launch
/// leg has been making these same calls on every launch since the feature
/// shipped.
fn rule_for_binding(
    binding: &KeyBinding,
    exe: &str,
    claims: &[(String, String)],
    exists: &dyn Fn(&std::path::Path) -> bool,
) -> ProfileRule {
    use crate::browser_profiles::{self, ProfileArg};

    let stem = launch_stem(exe);
    match browser_profiles::profile_arg_for(
        exe,
        binding.browser_profile_dir.as_deref(),
        browser_profiles::local_app_data().as_deref(),
        exists,
    ) {
        // A pinned binding never consults claims: it demands positive proof of
        // its OWN profile, which is strictly stronger than avoiding everyone
        // else's. That is also why no binding needs excluding from its own
        // claim list.
        //
        // Note `profile_arg_for`'s asymmetric polarity carries straight through
        // to the matcher, which is the point of reusing it: a profile that
        // could not be VERIFIED (Arc's MSIX alias has no derivable `User Data`
        // path) is still `Use`, so it is still `Pinned` here. Only a positively
        // located directory that positively lacks the folder is evidence.
        ProfileArg::Use(dir) => ProfileRule::Pinned(dir),
        // Nothing was pinned in the first place.
        ProfileArg::None => claims_rule(&stem, claims),
        // Pinned, and the folder is gone. The launch drops the switch; the
        // match must drop the demand, or the key never toggles again.
        ProfileArg::DroppedStale { .. } => claims_rule(&stem, claims),
    }
}

/// The rule a URL binding matches browser windows by, given the route its
/// LAUNCH leg has just chosen.
///
/// THE INVARIANT, and the only thing this function is: **the profile the match
/// leg demands is exactly the `--profile-directory` the launch leg will pass,
/// and nothing at all when the launch will pass none.** Pure and named so that
/// invariant can be asserted in a test instead of trusted — it was violated
/// once already, silently, by three inline arms that each looked reasonable.
///
/// NOTE WHAT IS ABSENT: `claims`. A URL binding does NOT decline a window
/// because another binding pinned that profile, and the app leg does. The two
/// legs differ because their candidate sets differ. This matcher has already
/// required the window's title to contain the binding's own site keyword or
/// host, so the worst `Any` can do here is toggle a window showing this very
/// key's website — which is the behaviour that shipped in 1.0.86 and which the
/// owner uses daily. `try_focus_or_minimize` has no such second discriminator:
/// there, EVERY window of the executable is a candidate, which is precisely the
/// gap the owner's unpinned rule was decided to close.
///
/// GENERALISES (and deliberately not implemented in this run — see the scope
/// note on `window_profile_evidence`): the shape here is "identity = whatever
/// the launch leg passes as an argument". Two `.lnk` shortcuts with different
/// arguments, two Windows Terminal profiles and two VS Code workspaces all
/// collide the same way and would all be fixed by deriving the match rule from
/// the launch plan, as these two functions now do for browser profiles.
fn url_rule_for_route(
    route: &crate::browser_profiles::BrowserRoute,
    binding: &KeyBinding,
    exists: &dyn Fn(&std::path::Path) -> bool,
) -> ProfileRule {
    use crate::browser_profiles::{self, BrowserRoute, ProfileArg};

    match route {
        BrowserRoute::Specific { exe } => match browser_profiles::profile_arg_for(
            exe,
            binding.browser_profile_dir.as_deref(),
            browser_profiles::local_app_data().as_deref(),
            exists,
        ) {
            // The launch will pass `--profile-directory=d`, so only that
            // profile's window is this key's.
            ProfileArg::Use(d) => ProfileRule::Pinned(d),
            // Nothing pinned, or a pin whose folder has been deleted. Either
            // way the launch passes no switch and the tab lands in whichever
            // profile that browser used last — unknowable from here, so
            // demanding any particular one could only ever refuse the right
            // window and open a duplicate tab instead.
            ProfileArg::None | ProfileArg::DroppedStale { .. } => ProfileRule::Any,
        },
        // Both of these launch through `run_browser`, which passes no profile
        // switch at all and raises with `ProfileRule::Any` for this same
        // reason (see its `raise_after_launch` call). A binding that pins no
        // browser cannot pin a profile that means anything.
        BrowserRoute::Default | BrowserRoute::PinnedBrowserMissing { .. } => ProfileRule::Any,
    }
}

/// COM, initialised for one match pass and released again.
///
/// `SHGetPropertyStoreForWindow` needs COM on the calling thread, and the stem
/// matcher runs on the engine thread, which may or may not already have it.
/// Uninitialised ONLY when our own init was the one that took: an
/// `RPC_E_CHANGED_MODE` means another apartment already owns this thread and
/// calling `CoUninitialize` would decrement somebody else's count. Same idiom
/// `run_browser` already uses, on a thread `aumid_focus_or_minimize` has been
/// initialising and uninitialising for months.
#[cfg(windows)]
struct ComGuard(bool);

#[cfg(windows)]
impl ComGuard {
    unsafe fn new() -> Self {
        use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
        ComGuard(CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok())
    }

    /// Only the profile-aware rules read window properties, so only they pay
    /// for COM. `ProfileRule::Any` - every binding that existed before this
    /// feature - gets `None` and touches nothing.
    unsafe fn for_rule(rule: &ProfileRule) -> Option<Self> {
        match rule {
            ProfileRule::Any => None,
            _ => Some(ComGuard::new()),
        }
    }
}

#[cfg(windows)]
impl Drop for ComGuard {
    fn drop(&mut self) {
        if self.0 {
            unsafe { windows::Win32::System::Com::CoUninitialize() };
        }
    }
}

/// Read ONE string property off a window's property store.
///
/// Factored out of `aumid_focus_or_minimize`'s callback rather than written a
/// second time: there is exactly one `SHGetPropertyStoreForWindow` +
/// `PropVariantToStringAlloc` + `CoTaskMemFree` sequence in this app, and both
/// the Store-app matcher and the browser-profile matcher go through it.
///
/// MUST run with COM initialised on the calling thread. Returns the string in
/// its ORIGINAL case - every comparison normalises, and lowercasing here would
/// throw away the literal folder name the launch leg needs to stay diff-able
/// against. A failure at any step is `None`: one window's unreadable property
/// must never abort an enumeration (PROBLEM 79's rule).
///
/// The `PROPVARIANT` frees itself on drop (windows-core 0.58 implements `Drop`
/// for it); only the `PropVariantToStringAlloc` buffer is ours to free.
#[cfg(windows)]
unsafe fn window_string_property(
    hwnd: windows::Win32::Foundation::HWND,
    pkey: &windows::Win32::UI::Shell::PropertiesSystem::PROPERTYKEY,
) -> Option<String> {
    use windows::Win32::System::Com::StructuredStorage::PropVariantToStringAlloc;
    use windows::Win32::UI::Shell::PropertiesSystem::{
        IPropertyStore, SHGetPropertyStoreForWindow,
    };

    let store: IPropertyStore = SHGetPropertyStoreForWindow(hwnd).ok()?;
    let pv = store.GetValue(pkey).ok()?;
    let pwstr = PropVariantToStringAlloc(&pv).ok()?;
    let value = pwstr.to_string().unwrap_or_default();
    windows::Win32::System::Com::CoTaskMemFree(Some(pwstr.0 as *mut _));
    Some(value)
}

/// The fmtid both AppUserModel properties share:
/// {9F4C2855-9F79-4B39-A8D0-E1D42DE1D5F3}. Hand-rolled like the AUMID key
/// below it, so no new `windows` crate feature has to be kept in sync
/// (PROBLEM 30's class of failure).
///   pid 2 = PKEY_AppUserModel_RelaunchCommand
///   pid 5 = PKEY_AppUserModel_ID
#[cfg(windows)]
const PKEY_APPUSERMODEL_FMTID: windows::core::GUID =
    windows::core::GUID::from_u128(0x9F4C2855_9F79_4B39_A8D0_E1D42DE1D5F3);

/// What this window says about which Chromium profile it belongs to.
///
/// RelaunchCommand (pid 2) is PRIMARY - it carries the LITERAL folder name,
/// `Profile 1`, space included. The AUMID (pid 5) is CORROBORATION and it
/// STRIPS THE SPACE, `Profile1`, which is why every comparison goes through
/// `same_profile_dir` and never through `==`.
///
/// Measured cost 0.026 ms per property read, so ~0.05 ms per window here, and
/// only for bindings whose rule is not `Any`.
#[cfg(windows)]
unsafe fn window_profile_evidence(hwnd: windows::Win32::Foundation::HWND) -> WindowProfile {
    use crate::browser_profiles::{
        profile_dir_from_relaunch_command, profile_token_from_aumid, same_profile_dir,
    };
    use windows::Win32::UI::Shell::PropertiesSystem::PROPERTYKEY;

    let relaunch = window_string_property(
        hwnd,
        &PROPERTYKEY { fmtid: PKEY_APPUSERMODEL_FMTID, pid: 2 },
    )
    .unwrap_or_default();

    let from_relaunch = profile_dir_from_relaunch_command(&relaunch);

    // The switch is THERE but no value could be read out of it. Refuse, rather
    // than fall through to NoProfileNamed: a window that DOES name a profile we
    // merely failed to parse is precisely the window we must not touch, and
    // this is what stops a parser miss from ever being mistaken for a
    // default-profile window.
    if from_relaunch.is_none() && relaunch.to_ascii_lowercase().contains("--profile-directory")
    {
        return WindowProfile::Unknown;
    }

    let from_aumid = profile_token_from_aumid(
        &window_string_property(
            hwnd,
            &PROPERTYKEY { fmtid: PKEY_APPUSERMODEL_FMTID, pid: 5 },
        )
        .unwrap_or_default(),
    );

    match (from_relaunch, from_aumid) {
        // Two properties of the same window disagreeing has never been seen.
        // If it ever happens, contradictory evidence is not evidence.
        (Some(rc), Some(au)) if !same_profile_dir(&rc, &au) => {
            log::warn!(
                "profile_match: {hwnd:?} contradicts itself - RelaunchCommand says {rc:?}, \
                 AUMID says {au:?}. Treating it as unidentified and leaving it alone."
            );
            WindowProfile::Unknown
        }
        (Some(rc), _) => WindowProfile::Named(rc),
        // Corroboration standing in for the primary: this is what rescues a
        // window whose relaunch command omits the switch but whose AUMID still
        // names the profile.
        (None, Some(au)) => WindowProfile::Named(au),
        (None, None) if !relaunch.trim().is_empty() => WindowProfile::NoProfileNamed,
        _ => WindowProfile::Unknown,
    }
}

#[cfg(windows)]
use winreg::{enums::HKEY_LOCAL_MACHINE, enums::HKEY_CURRENT_USER, RegKey};

/// What the cascade actually did — the toast must tell the truth about a
/// fallback, or the user cannot tell why "the wrong app" opened (2026-08-10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CascadeOutcome {
    /// The active profile's own binding acted.
    Primary,
    /// The active binding failed; the FOUNDERS binding for this key acted.
    Fallback,
    /// Nothing could be focused or launched at all.
    Failed,
}

/// Main cascade logic: focus → minimize toggle → launch.
/// Mirrors V11 SmartCascade(TargetApp, TargetWeb, FoundersApp, FoundersWeb).
pub fn smart_cascade(
    binding: &KeyBinding,
    fallback: Option<&KeyBinding>,
    claims: &[(String, String)],
    app_handle: Option<tauri::AppHandle>,
) -> CascadeOutcome {
    if let Some(app) = &binding.app {
        // THE GAP THAT USED TO BE HERE, closed 2026-08-27. The comment this
        // replaces said the focus/minimize match was by window title and
        // process name only and could not tell which PROFILE's window it was
        // grabbing, and that it would be fixed after the feature had shipped
        // and been used for a while. It shipped, it was used, and on the very
        // first day two Brave bindings collided: Space+N minimised Space+B's
        // window and never launched its own profile at all.
        //
        // What has NOT changed, because the owner's 2026-08-26 decision still
        // stands: this leg still runs FIRST, and a profile-only binding still
        // focuses-then-minimises on a second press exactly like every other app
        // binding. All that changed is WHICH window this leg is allowed to
        // touch.
        let rule = rule_for_binding(binding, app, claims, &real_fs());

        // ONE match leg for every shape of app binding (PROBLEM 216). This used
        // to be an `if is_shell_target(app) { aumid… } else { stem… }` ladder,
        // and the gap was everything that fitted NEITHER description: a
        // protocol-URI binding launched through a third route in
        // `launch_app_inner` that no branch of this ladder could see, so it
        // re-launched on every press for months while every fix went into the
        // matchers it never reached. `app_focus_or_minimize` derives the
        // identity from the SAME resolution the launch will perform.
        if app_focus_or_minimize(app, &rule) {
            return CascadeOutcome::Primary;
        }
        if launch_binding_app(binding, app, &rule, app_handle.clone()) {
            return CascadeOutcome::Primary;
        }
    }

    if let Some(url) = &binding.web_url {
        // Toggle an already-open browser window showing this site BEFORE
        // launching — otherwise every press opens another duplicate tab.
        // `url_focus_or_minimize` now resolves the browser and the profile the
        // same way the launch leg does, so the window it looks for and the
        // window it would open can no longer disagree (they did: a URL pinned
        // to Chrome went looking through the DEFAULT browser's windows).
        if url_focus_or_minimize(url, binding) { return CascadeOutcome::Primary; }
        if open_binding_url(binding, url, claims, app_handle.clone()) { return CascadeOutcome::Primary; }
    }

    // Fallback to the Founders binding for this key. This is a DELIBERATE
    // feature (owner's design): if the active profile's binding can't launch —
    // empty key, or the app isn't installed on this machine — try the
    // Founders entry so a sensible default still fires instead of nothing.
    if let Some(fb) = fallback {
        log::warn!(
            "cascade: active profile's binding failed (app={:?}, url={:?}) — falling back to the FOUNDERS binding for this key",
            binding.app, binding.web_url
        );
        // The fallback binding is dispatched through the SAME helpers, reading
        // ITS OWN browser fields — a Founders entry could one day pin a profile
        // too, and an arm that quietly ignored those fields would be a bug
        // nobody notices until it matters.
        if let Some(app) = &fb.app {
            // The rule is derived from `fb`, never from `binding` — the Founders
            // arm reads its OWN browser fields, exactly as it already did for
            // the launch leg.
            let rule = rule_for_binding(fb, app, claims, &real_fs());
            // The SAME single match leg as the primary arm — the two ladders
            // that used to be written out here are exactly the pair PROBLEM 216
            // found drifting.
            if app_focus_or_minimize(app, &rule) { return CascadeOutcome::Fallback; }
            if launch_binding_app(fb, app, &rule, app_handle.clone()) {
                return CascadeOutcome::Fallback;
            }
        }
        if let Some(url) = &fb.web_url {
            if url_focus_or_minimize(url, fb) { return CascadeOutcome::Fallback; }
            if open_binding_url(fb, url, claims, app_handle.clone()) { return CascadeOutcome::Fallback; }
        }
    }
    CascadeOutcome::Failed
}

// ---------------------------------------------------------------------------
// Browser + profile dispatch (2026-08-26)
// ---------------------------------------------------------------------------
//
// Both helpers below exist so there is exactly ONE place that decides whether a
// key press touches the browser-profile machinery, rather than that decision
// being repeated in the primary arm and the Founders-fallback arm of
// `smart_cascade` where the two could drift.

/// Ask the real filesystem. Wrapped in a closure so the decision functions in
/// `browser_profiles` can be tested against a fake one.
fn real_fs() -> impl Fn(&std::path::Path) -> bool {
    |p: &std::path::Path| crate::browser_profiles::path_exists(p)
}

/// How to launch an APP binding — a browser bound with NO url ("just open
/// Brave's Studies profile") — and what to tell the user afterwards.
struct AppLaunch {
    /// The `lpParameters` for `shell_launch`. `None` for every binding that
    /// does not pin a profile, which is every binding that existed before this
    /// feature — those reach `shell_launch` with a byte-identical argument
    /// list to the one they got then.
    params: Option<String>,
    /// The profile folder this launch will actually name on the command line —
    /// `None` when no switch is passed, INCLUDING when a pin was dropped as
    /// stale. Carried explicitly rather than re-derived, because it is what
    /// `raise_rule_for_launch` needs and re-deriving it is how the raise and
    /// the launch would come to disagree.
    profile_dir: Option<String>,
    /// Reason 2 of the two fallbacks, if it applies: the pinned profile folder
    /// was deleted, so the browser opens its own default profile instead. Held
    /// rather than raised immediately, because "Brave opened" must only be
    /// said once Brave has actually opened.
    stale_profile: Option<String>,
}

/// What the POST-LAUNCH watcher looks for, given what the launch actually put
/// on the command line.
///
/// THIS ANSWERS A DIFFERENT QUESTION FROM THE MATCH RULE, and conflating the
/// two is what produced a review finding on 2026-08-27. The match rule asks
/// *may this key minimise this window?* — where a wrong yes is the reported
/// bug, so it must be provable-only. This asks *which window did the launch I
/// just performed create?* — where a wrong **no** means refusing to raise the
/// very window we just opened, and the user watches their browser come up
/// behind whatever they were looking at (PROBLEM 170's symptom, back again).
///
/// So it describes the LAUNCH, not the binding:
///   · a switch was passed  -> only that profile's window can be the one
///   · a pin was DROPPED as stale -> the browser opened its own default
///     profile, which nobody can predict from here, so any window of it will
///     do. This is the case the URL leg used to get wrong: it asked for
///     `claims_rule`, which can decline the window the launch just made
///     because a DIFFERENT binding pins that profile, and then the watcher
///     polls for eight seconds and raises nothing.
///   · nothing was pinned at all -> the binding's own match rule, which is
///     `claims_rule` on the app leg. Unchanged, and deliberately not widened
///     to `Any`: an unpinned key that just launched has no business pulling
///     another binding's window forward, and the window it did open is already
///     coming up in front from `AllowSetForegroundWindow`.
///
/// One function for both legs, for the reason `cache_key` is one function: two
/// sites deriving the same answer separately is a bug waiting for an edit.
fn raise_rule_for_launch(
    passed_profile: Option<&str>,
    pin_dropped_as_stale: bool,
    unpinned: &ProfileRule,
) -> ProfileRule {
    match (passed_profile, pin_dropped_as_stale) {
        (Some(d), _) => ProfileRule::Pinned(d.to_string()),
        (None, true) => ProfileRule::Any,
        (None, false) => unpinned.clone(),
    }
}

fn app_launch_plan(binding: &KeyBinding) -> AppLaunch {
    use crate::browser_profiles::{self, ProfileArg};

    let none = AppLaunch { params: None, profile_dir: None, stale_profile: None };
    let (Some(exe), Some(dir)) = (
        binding.app.as_deref(),
        binding.browser_profile_dir.as_deref(),
    ) else {
        return none;
    };

    match browser_profiles::profile_arg_for(
        exe,
        Some(dir),
        browser_profiles::local_app_data().as_deref(),
        &real_fs(),
    ) {
        ProfileArg::None => none,
        ProfileArg::Use(d) => AppLaunch {
            params: browser_profiles::build_launch_params(Some(&d), None),
            profile_dir: Some(d),
            stale_profile: None,
        },
        ProfileArg::DroppedStale { dir, searched } => {
            // Same voice and detail level as PROBLEM 116's versioned-path
            // repair: say what was asked for, what was actually found, and what
            // is happening instead — so a log reader never has to guess whether
            // the app misbehaved or the world changed underneath it.
            log::warn!(
                "cascade: profile '{dir}' is gone — it was deleted inside the browser \
                 (looked for it in '{}'). Launching {exe} WITHOUT --profile-directory, \
                 so it opens its own default profile instead.",
                searched.display()
            );
            AppLaunch {
                params: None,
                // The switch was DROPPED — this launch names no profile, and
                // the post-launch raise must be told so rather than inferring
                // it from `stale_profile` being a message.
                profile_dir: None,
                stale_profile: Some(browser_profiles::stale_profile_toast(
                    binding.browser_profile_name.as_deref(),
                    &dir,
                    exe,
                )),
            }
        }
    }
}

/// Launch an APP binding, and raise the stale-profile message if that is what
/// happened.
///
/// One helper rather than the same three lines at all four `launch_app` call
/// sites in `smart_cascade` — the primary and Founders arms have drifted apart
/// before, and the profile fields are read from whichever binding is being
/// dispatched, never from "the" binding.
///
/// A binding that pins no profile reaches `launch_app` with exactly the
/// arguments it did before this feature existed.
fn launch_binding_app(
    binding: &KeyBinding,
    app: &str,
    rule: &ProfileRule,
    app_handle: Option<tauri::AppHandle>,
) -> bool {
    let plan = app_launch_plan(binding);
    // What the raise looks for is decided by what this launch is about to put
    // on the command line, through the same function the URL leg uses — see
    // `raise_rule_for_launch`. Behaviour is unchanged from the shape this
    // replaced (`if stale { Any } else { rule.clone() }`), because `rule` and
    // `plan` are now both derived from the one `profile_arg_for` call: a live
    // pin makes them both `Pinned(d)`, and an unpinned binding makes them both
    // `claims_rule`. What changed is that there is one function saying it.
    let raise_rule =
        raise_rule_for_launch(plan.profile_dir.as_deref(), plan.stale_profile.is_some(), rule);
    let launched = launch_app(app, plan.params.as_deref(), &raise_rule, app_handle);
    if launched {
        if let Some(msg) = plan.stale_profile {
            notify(&msg);
        }
    }
    launched
}

/// Open `url`, either in the OS default browser (unchanged) or in the specific
/// browser+profile this binding pins.
///
/// **THE HARD REQUIREMENT LIVES HERE.** The owner said it twice: *"MAKE SURE
/// THE DEFAULT BROWSER LAUNCHES FROM URL IF NOT EXPLICITLY SET TO SPECIFIC."*
/// `route_for` — which consults
/// `browser_profiles::should_use_specific_browser` — is asked FIRST, and the
/// `Default` arm calls `run_browser(url, app_handle)` exactly as this code did
/// before the feature existed: same function, same arguments, no new logic in
/// or around that call. There is deliberately no other route to `run_browser`
/// from `smart_cascade`, and a test in `browser_profiles.rs` reads this source
/// to prove the ordering has not been reversed.
fn open_binding_url(
    binding: &KeyBinding,
    url: &str,
    claims: &[(String, String)],
    app_handle: Option<tauri::AppHandle>,
) -> bool {
    use crate::browser_profiles::{self, BrowserRoute, ProfileArg};

    let exe = match browser_profiles::route_for(binding, &real_fs()) {
        // Every binding that exists today, and every new one where the user
        // never opens the profile picker.
        BrowserRoute::Default => return run_browser(url, app_handle),

        // Pinned, but the browser has been uninstalled since. Falling back to
        // the default browser beats leaving a dead key — the user cannot tell a
        // key that does nothing from an app that is broken.
        BrowserRoute::PinnedBrowserMissing { exe } => {
            log::warn!(
                "cascade: this key is pinned to '{exe}', which is no longer on this \
                 machine — the browser was uninstalled or moved. Opening {url} in the \
                 DEFAULT browser instead. The BINDING IS NOT REWRITTEN: reinstalling that \
                 browser must simply start working again (design handoff §5)."
            );
            let launched = run_browser(url, app_handle);
            if launched {
                // REASON 1 of 2. Only after a successful launch — "opened in
                // Edge" must not be said about a launch that did not happen,
                // and a failure already gets the cascade's own "❌ … could not
                // be opened" from the engine.
                notify(&browser_profiles::pinned_browser_missing_toast(
                    &exe,
                    browser_stem().as_deref(),
                ));
            }
            return launched;
        }

        BrowserRoute::Specific { exe } => exe,
    };

    // `stale_profile` carries the DIFFERENT failure: the browser is fine, the
    // profile folder inside it is not. Held until the launch has actually
    // happened, for the same reason as reason 1 above.
    let mut stale_profile: Option<String> = None;
    let profile_dir = match browser_profiles::profile_arg_for(
        &exe,
        binding.browser_profile_dir.as_deref(),
        browser_profiles::local_app_data().as_deref(),
        &real_fs(),
    ) {
        ProfileArg::None => None,
        ProfileArg::Use(d) => Some(d),
        ProfileArg::DroppedStale { dir, searched } => {
            log::warn!(
                "cascade: profile '{dir}' is gone — it was deleted inside the browser \
                 (looked for it in '{}'). Opening {url} in {exe} WITHOUT \
                 --profile-directory, so it lands in that browser's own default profile.",
                searched.display()
            );
            stale_profile = Some(browser_profiles::stale_profile_toast(
                binding.browser_profile_name.as_deref(),
                &dir,
                &exe,
            ));
            None
        }
    };

    let params = browser_profiles::build_launch_params(profile_dir.as_deref(), Some(url));
    log::info!(
        "cascade: opening {url} in the SPECIFIC browser {exe} (params: {})",
        params.as_deref().unwrap_or("<none>")
    );
    // The post-launch raise must look for the SAME window the launch is about
    // to create — through the same function the app leg uses.
    //
    // REVIEW FINDING, 2026-08-27, real and fixed here. The comment this
    // replaces claimed that a dropped pin meant "the raise correctly stops
    // discriminating too", and it did not: BOTH `None` outcomes fell into
    // `claims_rule`, so after a stale pin was dropped the watcher could refuse
    // the window it had just caused to exist — because a DIFFERENT binding
    // pinned the profile the browser happened to open in — and then poll for
    // eight seconds and raise nothing. `launch_binding_app` had already worked
    // this out for the app leg; the two legs now say it in one place.
    let stem = launch_stem(&exe);
    let raise_rule = raise_rule_for_launch(
        profile_dir.as_deref(),
        stale_profile.is_some(),
        &claims_rule(&stem, claims),
    );
    let launched = shell_launch(&exe, params.as_deref(), app_handle);
    if launched {
        // Same post-launch raise every other route gets (PROBLEM 170).
        raise_after_launch(vec![stem], raise_rule);
        // REASON 2 of 2 — and note it is a DIFFERENT sentence naming DIFFERENT
        // things than reason 1: the profile that vanished and the browser that
        // opened anyway, versus the browser that vanished and the one that
        // opened instead. Collapsing them into one message is what left the
        // user guessing which half to fix.
        if let Some(msg) = stale_profile {
            notify(&msg);
        }
    }
    launched
}

/// Raise a toast from the cascade.
///
/// The AppHandle comes from the OnceLock accessor that `guide_hud` already
/// owns, NOT from a second copy threaded through these functions — the PiP
/// release path started doing exactly this on 2026-08-26 and this follows it.
/// `AppHandle::emit` is fire-and-forget onto the main event loop, so it is
/// safe from the engine thread; `None` only happens in the brief window before
/// `set_app_handle` has run, in which case there is no overlay to toast into
/// yet and the log line above is the whole record.
fn notify(msg: &str) {
    if let Some(handle) = crate::guide_hud::app_handle() {
        crate::show_toast(&handle, msg);
    }
}

use tauri::Emitter;

/// Launch anything the Windows shell can open — .exe, .lnk (arguments in the
/// shortcut survive), protocol URIs, documents — via ShellExecuteExW.
///
/// This replaced `std::process::Command::spawn`, which CANNOT execute .lnk
/// files (os error 193 "%1 is not a valid Win32 application", seen live with
/// a Swoosh.lnk binding), cannot open URIs at all, and errors with 740 on
/// exes whose manifest demands elevation. ShellExecute is what v11's AHK
/// `Run` used — this is the parity path.
#[cfg(windows)]
/// Launch via EXPLORER (medium integrity) instead of our elevated process.
///
/// PROBLEM 56 — the reason a tester's Space+letter "launched" apps that never
/// appeared. Spaceadom runs ELEVATED (the keyboard hook needs it). Chromium
/// browsers launched by an elevated parent de-elevate themselves by
/// re-launching through the shell, and on some machines that handoff dies
/// silently: ShellExecuteEx returns success, the process exits, no window is
/// ever created. The tester's log proved it — five `ShellExecute launched
/// brave.exe` lines, then `url_focus … Titles seen: []` showing ZERO Brave
/// windows existed seconds later.
///
/// The Microsoft-documented fix (Raymond Chen, "How can I launch an
/// unelevated process from my elevated process", + the ExecInExplorer SDK
/// sample) is to hand the launch to the DESKTOP's explorer via
/// IShellDispatch2::ShellExecute. Explorer runs unelevated at medium
/// integrity, so the app starts exactly as if the user double-clicked it —
/// browsers, .lnk shortcuts, URLs and shell: AUMIDs all behave normally.
#[cfg(windows)]
fn shell_launch_unelevated(file: &str, params: Option<&str>) -> bool {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    // `explorer.exe <target>` hands the launch to the ALREADY-RUNNING desktop
    // shell, which is unelevated at medium integrity, so the target starts
    // exactly as if the user had double-clicked it. explorer accepts exe
    // paths, .lnk shortcuts, URLs and `shell:AppsFolder\<AUMID>` alike.
    //
    // Chosen over the IShellDispatch2/ExecInExplorer COM chain deliberately:
    // same de-elevation, a fraction of the surface area, and no extra
    // `windows` crate features to keep in sync.
    //
    // NOTE: explorer.exe always exits ~immediately after handing off, so its
    // exit status says nothing about whether the app started — never treat it
    // as proof (that mistake is why a log once read "launched" for apps that
    // never appeared).
    let mut cmd = std::process::Command::new("explorer.exe");
    if let Some(p) = params.filter(|p| !p.is_empty()) {
        // explorer takes ONE argument; fold params into the target when the
        // caller supplied them (URLs passed to a browser exe).
        cmd.arg(format!("{file} {p}"));
    } else {
        cmd.arg(file);
    }
    match cmd.creation_flags(CREATE_NO_WINDOW).spawn() {
        Ok(_) => {
            log::info!("cascade: launched {file} UNELEVATED via explorer (as a double-click would)");
            true
        }
        Err(e) => {
            log::warn!(
                "cascade: unelevated launch failed for {file}: {e} — falling back to direct ShellExecute"
            );
            false
        }
    }
}

#[cfg(not(windows))]
fn shell_launch_unelevated(_f: &str, _p: Option<&str>) -> bool { false }

/// Re-resolve a saved path whose app has updated itself into a new folder.
///
/// PROBLEM 116, found live 2026-08-16. Space+D stopped opening Discord. The
/// binding held `...\Discord\app-1.0.9251\Discord.exe`; on disk was
/// `...\Discord\app-1.0.9253\Discord.exe`. Discord had updated, and the saved
/// path died with the folder it named. The user sees "the shortcut broke",
/// which is the wrong story — the shortcut is fine, the target moved.
///
/// This is not a Discord quirk. It is the Squirrel installer's layout, used by
/// Slack, Teams (classic), GitHub Desktop and Signal among others: the exe
/// lives in `<App>\app-<version>\` and EVERY self-update creates a new one.
/// Any absolute path saved into such a folder is guaranteed to break, on every
/// machine, at an unpredictable future date. Re-resolving is the only fix that
/// stays fixed.
///
/// Returns `(target, params)` ready for `shell_launch`, or `None` when the app
/// really is gone.
///
/// GENERALISE THIS: an absolute path stored today is a guess about tomorrow's
/// filesystem. Anything that stores one needs a recovery path, not just an
/// error message.
#[cfg(windows)]
fn repair_versioned_path(dead: &std::path::Path) -> Option<(String, Option<String>)> {
    use std::path::PathBuf;

    // Find the `app-<version>` ancestor. Only this exact shape is accepted —
    // matching looser patterns risks launching an unrelated executable.
    let comps: Vec<_> = dead.components().collect();
    let idx = comps.iter().position(|c| {
        c.as_os_str()
            .to_string_lossy()
            .to_ascii_lowercase()
            .starts_with("app-")
    })?;
    let base: PathBuf = comps[..idx].iter().collect();
    let tail: PathBuf = comps[idx + 1..].iter().collect();

    // Newest sibling `app-*`, chosen by modification time rather than by name:
    // version strings stop sorting lexicographically the moment a component
    // reaches double digits (app-1.0.9 vs app-1.0.10).
    let mut newest: Option<(std::time::SystemTime, PathBuf)> = None;
    for entry in std::fs::read_dir(&base).ok()?.flatten() {
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        if !name.starts_with("app-") || !entry.path().is_dir() {
            continue;
        }
        let t = entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        if newest.as_ref().is_none_or(|(best, _)| t > *best) {
            newest = Some((t, entry.path()));
        }
    }
    if let Some((_, dir)) = newest {
        let candidate = dir.join(&tail);
        if candidate.exists() {
            return Some((candidate.to_string_lossy().into_owned(), None));
        }
    }

    // Squirrel's own stable entry point. This is what the Start Menu shortcut
    // runs, it has never moved, and it will survive every future update — so
    // it is the better answer even though it is the fallback.
    let updater = base.join("Update.exe");
    if updater.exists() {
        let exe = dead.file_name()?.to_string_lossy().into_owned();
        return Some((
            updater.to_string_lossy().into_owned(),
            Some(format!("--processStart {exe}")),
        ));
    }
    None
}

fn shell_launch(file: &str, params: Option<&str>, app_handle: Option<tauri::AppHandle>) -> bool {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
    use windows::Win32::System::Threading::{WaitForSingleObject, INFINITE};
    use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
    use windows::Win32::UI::WindowsAndMessaging::{AllowSetForegroundWindow, ASFW_ANY, SW_SHOWNORMAL};

    // PROBLEM 225 — clear FIRST, on every path through this function. A stale
    // PID is the only way this could ever hand `raise_after_launch` a wrong
    // identity, and clearing on entry makes that impossible by construction:
    // every early return below (elevated relaunch, ShellExecute refusal, a
    // launch that creates no process) leaves 0, which every consumer reads as
    // "unknown".
    LAST_LAUNCH_PID.store(0, std::sync::atomic::Ordering::Relaxed);

    unsafe {
        // ShellExecuteEx wants COM (.lnk resolution goes through the shell).
        // Ignore the result: RPC_E_CHANGED_MODE just means the thread already
        // has a concurrency model, which is fine.
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

        // When elevated, prefer explorer's unelevated launch (PROBLEM 56).
        // The direct path below stays as the fallback for machines where the
        // desktop shell COM chain is unavailable.
        if crate::startup::is_elevated() && shell_launch_unelevated(file, params) {
            if let Some(app) = &app_handle {
                let _ = app.emit("app-launched", &file.to_string());
            }
            return true;
        }

        let file_h = HSTRING::from(file);
        let verb_h = HSTRING::from("open");
        let params_h = params.map(HSTRING::from);

        let mut sei = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS,
            lpVerb: PCWSTR(verb_h.as_ptr()),
            lpFile: PCWSTR(file_h.as_ptr()),
            lpParameters: params_h
                .as_ref()
                .map(|h| PCWSTR(h.as_ptr()))
                .unwrap_or(PCWSTR::null()),
            nShow: SW_SHOWNORMAL.0,
            ..Default::default()
        };

        // PROBLEM 170, half 1 — hand our foreground privilege to whatever the
        // shell is about to start, so its first window may come up in front on
        // its own. MUST be immediately before the shell call: the grant is
        // consumed by the next process the caller creates and times out
        // quickly. Without it Windows treats the new process as a background
        // launch and denies it foreground, which is why apps "do not come up".
        // Failure is not worth reporting loudly — half 2 (raise_after_launch)
        // covers the same ground from the other side.
        // COMMIT 0 (PROBLEM 225) — info!, not debug!. Whether the OS handed our
        // foreground privilege to the new process is HALF of why an app comes
        // up in front, and at debug! it was invisible in every report the owner
        // has ever sent. One line per launch, ~20 launches a day.
        if let Err(e) = AllowSetForegroundWindow(ASFW_ANY) {
            log::info!("cascade: AllowSetForegroundWindow refused ({e}) — the watcher will raise it instead");
        }

        if let Err(e) = ShellExecuteExW(&mut sei) {
            log::warn!("cascade: ShellExecute failed for {file}: {e}");
            return false;
        }

        // PROBLEM 54 — "ShellExecute launched" DOES NOT MEAN A WINDOW APPEARED.
        // ShellExecuteEx returns success for activations that create no
        // process at all, and hInstApp still carries the legacy error code.
        // This line previously claimed success unconditionally, and I read a
        // tester's log and told the user apps "were opening" when nothing had
        // appeared on his screen. Never report a launch without reporting
        // whether a PROCESS was actually created.
        //
        // hInstApp <= 32 is the classic ShellExecute error range:
        //   2 = file not found, 3 = path not found, 5 = ACCESS DENIED,
        //   26/27/31/32 = sharing/assoc/no-app failures.
        // ACCESS DENIED (5) is the one to watch: we run ELEVATED, and an
        // elevated process launching a per-user app can be refused outright.
        let inst = sei.hInstApp.0 as usize;
        let pid_created = !sei.hProcess.is_invalid() && !sei.hProcess.0.is_null();
        if inst <= 32 {
            log::warn!(
                "cascade: ShellExecute REFUSED {file} — hInstApp={inst} \
                 (2=not found, 3=bad path, 5=ACCESS DENIED, 31=no association). \
                 Nothing was opened."
            );
            return false;
        }
        log::info!(
            "cascade: ShellExecute accepted {file} (hInstApp={inst}, process_created={pid_created})"
        );

        let name = file.to_string();
        if let Some(app) = &app_handle {
            let _ = app.emit("app-launched", &name);
        }

        // SEE_MASK_NOCLOSEPROCESS gives us the child handle when the shell
        // actually created a process (not for DDE/Store activations) — wait on
        // it off-thread so the frontend still gets its app-closed event.
        let hproc = sei.hProcess;
        if !hproc.is_invalid() && !hproc.0.is_null() {
            // PROBLEM 225 — POSITIVE IDENTITY for the post-launch watcher, read
            // BEFORE the handle is handed to the waiter thread that closes it.
            // This is the one thing the exe-stem matcher can never work out on
            // its own: a `.lnk`, a `whatsapp://` handler and a Squirrel
            // `Update.exe` stub all end up running under a different name than
            // the binding says. `GetProcessId` on a handle we still own cannot
            // race the close below (same statement order, same thread).
            let created_pid =
                windows::Win32::System::Threading::GetProcessId(hproc);
            LAST_LAUNCH_PID.store(created_pid, std::sync::atomic::Ordering::Relaxed);
            let raw = hproc.0 as isize;
            std::thread::spawn(move || {
                let h = windows::Win32::Foundation::HANDLE(raw as *mut _);
                let _ = WaitForSingleObject(h, INFINITE);
                let _ = CloseHandle(h);
                if let Some(app) = &app_handle {
                    let _ = app.emit("app-closed", &name);
                }
            });
        }
        true
    }
}

#[cfg(not(windows))]
fn shell_launch(_file: &str, _params: Option<&str>, _app_handle: Option<tauri::AppHandle>) -> bool {
    false
}

// ---------------------------------------------------------------------------
// PROBLEM 225 — the post-launch raise's decision logic, as PURE functions
// ---------------------------------------------------------------------------
//
// These sit outside `#[cfg(windows)]` and take no HWNDs on purpose: the two
// judgements that decide whether the owner's app comes to the front are the
// part that was wrong for six days, and they were previously unreachable by a
// test because they were three lines welded to `GetForegroundWindow`. The
// watcher below now only gathers facts; these decide.

/// How long after a launch NOTHING may stand the watcher down.
///
/// See `raise_after_launch`'s doc comment for the derivation. Short version:
/// the Space+key press IS the instruction, a raise inside a beat of it is the
/// feature working, and this file's own cold-start measurements (Brave ~500 ms,
/// VLC ~1 s) sit inside this window. **A proposal, not a measured constant** —
/// it is the number 22 of 29 false "you started typing" stand-downs in the
/// owner's 2026-08-25..31 log fall under. Moving it is the owner's call.
const RAISE_GRACE_MS: u64 = 1_500;

/// Should the watcher stand down because a key went down?
///
/// `latest_tick` must come from `hook::last_user_typing_tick()` and NOT from
/// `hook::last_keyboard_event_tick()` — the whole of PROBLEM 225 is that the
/// second one counts our own injections, the combo's own releases and the
/// watchdog's idle re-stamp as "the user typed".
fn typing_stand_down(since_launch_ms: u64, baseline_tick: u64, latest_tick: u64) -> bool {
    since_launch_ms >= RAISE_GRACE_MS && latest_tick > baseline_tick
}

/// What the watcher may conclude from a foreground window that is not ours.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ForeignVerdict {
    /// Say nothing, keep polling.
    KeepPolling,
    /// A different application genuinely owns the screen. Raise nothing.
    StandDown,
    /// The foreground changed but its process could not be named. **Not** a
    /// stand-down: an unproven window is not evidence about the user, and this
    /// function's whole job is to stop asserting things about him.
    ///
    /// READ THIS BEFORE "RESTORING" ANYTHING (review finding, 2026-08-31).
    /// This verdict does NOT mean "raise nothing". The poll loop runs step 2
    /// (find the target and raise it) BEFORE step 3 (this verdict) and
    /// `return`s from step 2, so `Unidentified` means *keep polling AND still
    /// raise the moment the target appears*. The earlier wording here and in
    /// `raise_after_launch`'s doc said the deadline would "expire having raised
    /// nothing", which is true only in the sub-case where the target never
    /// appears at all.
    ///
    /// That IS the intended trade — the Space+key press is the instruction, the
    /// only window we can ever raise is the one this launch just started, and
    /// an unnameable third window is not evidence that the owner changed his
    /// mind. It is written down here because a contract the code does not keep
    /// is an invitation to "fix" the code to match it.
    Unidentified,
}

/// Decide whether a changed foreground window means the user moved on.
///
/// `launched_pid` is what `ShellExecuteEx` reported through
/// `SEE_MASK_NOCLOSEPROCESS`, or 0 when the shell created no process at all
/// (folders, DDE and Store activations, and the unelevated-relaunch path).
/// Zero is treated strictly as "unknown" and never matches a real PID —
/// getting that backwards would make every launch look like its own target.
///
/// NATIVE_SAFETY, explicitly: this identity NEVER authorises touching a window.
/// Its only two outcomes are "keep polling" and "stop" — the decision to
/// `ShowWindow`/`SetForegroundWindow` still belongs entirely to
/// `find_window_by_exe_stem` -> `enum_callback`, which keeps the explorer.exe
/// `CabinetWClass` POSITIVE filter and the empty-title rule. Proving a process
/// is not a way past the class rule, and this function must never become one.
fn foreign_foreground_verdict(
    since_launch_ms: u64,
    fg_changed: bool,
    fg: Option<(u32, &str)>,
    launched_pid: u32,
    stems: &[String],
) -> ForeignVerdict {
    if !fg_changed || since_launch_ms < RAISE_GRACE_MS {
        return ForeignVerdict::KeepPolling;
    }
    let Some((fg_pid, fg_stem)) = fg else {
        return ForeignVerdict::Unidentified;
    };
    // Positive identity first: this IS the process we started. The stem matcher
    // cannot know that — a `.lnk`, a `whatsapp://` protocol handler and a
    // Squirrel `Update.exe` stub all end up running something else's name.
    if launched_pid != 0 && fg_pid == launched_pid {
        return ForeignVerdict::KeepPolling;
    }
    if stems.iter().any(|s| s.eq_ignore_ascii_case(fg_stem)) {
        return ForeignVerdict::KeepPolling;
    }
    ForeignVerdict::StandDown
}

/// PID of the process the last `shell_launch` actually created, 0 when it made
/// none. Written by `shell_launch` (both on entry, to clear a stale value, and
/// on success) and by `run_browser`; taken exactly once by `raise_after_launch`.
///
/// A STATIC rather than a return value because `shell_launch` is called from
/// eight sites and returns `bool` to all of them; threading an `Option<u32>`
/// through would touch every launch path in the file for one diagnostic. The
/// pairing is safe because the launch is the statement immediately before the
/// watcher spawn on the same (engine) thread, and because every entry point
/// stores 0 first — so the worst case is losing an identity, never inventing
/// one, and losing it only means we keep polling.
#[cfg(windows)]
static LAST_LAUNCH_PID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// `(pid, lowercase exe stem)` for the process that owns this window.
///
/// `None` for anything we cannot prove — pid 0, a process we may not open, a
/// failed image-name query. Callers must treat `None` as "unknown", never as
/// "not ours": that distinction is the whole of PROBLEM 225's part 1c.
#[cfg(windows)]
fn window_process_identity(hwnd: HWND) -> Option<(u32, String)> {
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return None;
        }
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 260];
        let mut size = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_FORMAT(0),
            windows::core::PWSTR(buf.as_mut_ptr()),
            &mut size,
        );
        let _ = windows::Win32::Foundation::CloseHandle(handle);
        if ok.is_err() {
            return None;
        }
        let path = String::from_utf16_lossy(&buf[..size as usize]);
        let stem = std::path::Path::new(&path)
            .file_stem()
            .map(|f| f.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if stem.is_empty() {
            return None;
        }
        Some((pid, stem))
    }
}

/// PROBLEM 170 — pull a JUST-LAUNCHED app's window to the front.
///
/// THE BUG. `force_foreground` was called from four places and every one of
/// them was inside a *focus an existing window* path. **Nothing whatsoever ran
/// after a launch.** So the CORE_AIM contract "If closed: launch the app" was
/// half-implemented: the process started, and whether its window ended up in
/// front was left entirely to Windows. Windows' answer is usually "no":
///
///   * Spaceadom is NOT the foreground process when a shortcut fires — our
///     hook swallowed the keystroke, so the OS never credited us with the
///     input that would grant foreground rights.
///   * A process launched by a background process inherits that refusal. Its
///     window opens BEHIND, or its taskbar button just flashes.
///   * Many apps restore their last session's window state, so one that was
///     closed while minimised comes back minimised.
///
/// The owner: *"when apps launched, they do not come up, they sometimes launch
/// minimized in the taskbar"* — and, asked directly, confirmed it happens
/// **only** when the app has to be launched, never when it is already running.
/// That is this gap exactly, and nothing else.
///
/// THE FIX IS TWO HALVES, and neither is sufficient alone.
///
/// 1. `AllowSetForegroundWindow(ASFW_ANY)` immediately before the shell call
///    (see `shell_launch` / `run_browser`). This is the documented way to hand
///    OUR foreground privilege to the process we are about to start, so its
///    first window is allowed to come up in front on its own. It only applies
///    to a process the shell creates for us, and it expires quickly — which is
///    why half 2 exists.
/// 2. This watcher. Cold-start latency is wildly variable (measured on this
///    machine: Brave ~500ms, VLC ~1s, Electron apps several seconds), so a
///    single delayed attempt would miss exactly the slow apps that fail today.
///
/// WHY IT STANDS DOWN, AND ON WHAT — REWRITTEN, PROBLEM 225.
///
/// The original rule came from the owner's own answer when offered the
/// alternatives: *"keep trying ~8s, but stand down if you touch anything."*
/// The intent behind it was never in doubt. The IMPLEMENTATION was, and six
/// days of his log say so: **57 of 79 raises stood down, and most of them were
/// standing down from the user's own keypress.**
///
/// The corrected framing, and it is the spec now: **the Space+key press IS the
/// instruction.** A window coming to the front a beat after the command that
/// asked for it is not focus theft — it is the feature working. Stand-down
/// exists for ONE case: the window arrived so late that the user has genuinely
/// moved on.
///
/// So there are two changes, and neither is a tuning knob:
///
///   * **A `RAISE_GRACE_MS` window during which nothing stands the watcher
///     down.** Inside it, the first window found is raised unconditionally.
///     1500 ms is not a measured constant — it is chosen because this file's
///     own cold-start measurements (Brave ~500 ms, VLC ~1 s, a few lines up)
///     sit inside it, it covers the first ~8 polls on the 500 + n×120 grid, and
///     it is the number **22 of 29** false "you started typing" stand-downs in
///     the owner's log fall under. It is the owner's call to move.
///
///   * **A signal that actually means "a human is typing".**
///     `hook::last_keyboard_event_tick()` never did. It is stamped for every
///     callback before the injected-input cookie test, AND from
///     `install_hooks` / `watchdog_check` off the hook path entirely — so the
///     Space-UP of the firing combo, the combo's own letter release, our own
///     injected Space and a watchdog tick all read as "the user typed". Fixing
///     that with a longer `SETTLE_MS` was never possible: the owner holds Space
///     to READ the Guide HUD, so his Space-up lands 600–1000 ms after the
///     launch and the measured false stand-downs cluster exactly there.
///     `hook::last_user_typing_tick()` (PROBLEM 225) excludes all four by
///     construction rather than by timing.
///
/// **"You switched to another window" got the same treatment.** As a bare
/// "the foreground HWND changed" test it was a first-window stand-down: the app
/// we launched putting up its OWN splash, or a File Explorer window opening for
/// a folder binding, both tripped it. It now requires all of: past the grace,
/// AND the foreground process is neither the PID `ShellExecuteEx` handed back
/// nor any of this launch plan's exe stems. If that process cannot be
/// identified at all we **do not** stand down — we keep polling, and if the
/// target window appears before the deadline we still raise it. An unproven
/// third window is not evidence about the user. (Only if the target never
/// appears does the deadline expire having raised nothing; the step order in
/// the loop below is find-and-raise FIRST, verdict second, and it returns from
/// the raise. See `ForeignVerdict::Unidentified` — the doc there used to claim
/// the stronger property and it was never true.)
///
/// **The messages state the observation, never an assertion about the user.**
/// "you started typing" and "you switched to another window" were wrong ~57
/// times in six days, and each one was a sentence blaming the owner for a bug.
/// They now report what was seen — which key, how long after the launch, which
/// process — so the next report is diagnosable in one grep.
///
/// Mouse MOVEMENT deliberately does not abort: drifting the pointer while an
/// app loads is not a decision.
///
/// It never minimises. The focus/minimise toggle belongs to the cascade's
/// other paths; this one only ever raises, so a race with an app that puts
/// itself in front cannot flip it into hiding the window it just opened.
///
/// PROFILE AWARENESS (2026-08-27). `find_window_by_exe_stem` reuses
/// `enum_callback`, so before this parameter existed a perfectly CORRECT launch
/// into Profile 2 could be followed by raising Profile 1's window — the same
/// blindness as the cascade's, one step later and harder to see, because the
/// launch itself was right. The fail-safe is the same as everywhere else and
/// costs nothing here: an unproven window is simply not found, the watcher
/// keeps polling, and it stands down at the deadline having raised NOTHING.
/// Raising nothing is a window that opens where the app put it. Raising the
/// wrong one is the bug.
///
/// PROBLEM 216 made this a LIST. The watcher used to be told one exe stem, and
/// that stem came from the binding string rather than from the resolution the
/// launch performed — so for a `whatsapp://` binding it polled for eight
/// seconds for a process called `whatsapp` while the program that had just
/// started was `WhatsApp.Root`, and for a Start-Menu `.lnk` it polls for the
/// shortcut's name rather than its target's. The stems now come from the same
/// `LaunchPlan` the launch itself used, in order, cheapest first.
#[cfg(windows)]
fn raise_after_launch(exe_stems: Vec<String>, rule: ProfileRule) {
    use windows::Win32::UI::WindowsAndMessaging::{IsIconic, ShowWindow, SW_RESTORE};

    let exe_stems: Vec<String> = exe_stems.into_iter().filter(|s| !s.is_empty()).collect();
    if exe_stems.is_empty() {
        return;
    }
    let exe_stem = exe_stems.join(" or ");

    const SETTLE_MS: u64 = 500; // give the shell a beat before the first poll
    const POLL_MS: u64 = 120;
    const GIVE_UP_MS: u64 = 8_000;

    // TAKE-ONCE (PROBLEM 225). `shell_launch` stamps the PID `ShellExecuteEx`
    // handed back and this is the only reader, so the value cannot be seen
    // twice. Read on the CALLING thread, before the watcher is spawned: the
    // launch that stamped it is the statement immediately above every call
    // site, so on this thread the pairing is deterministic. Zero means
    // "no process identity for this launch" — a folder, a DDE/Store
    // activation, or the unelevated-relaunch path — and every consumer below
    // treats zero as "unknown", never as a PID.
    let launched_pid = LAST_LAUNCH_PID.swap(0, std::sync::atomic::Ordering::Relaxed);

    // Kept for the spawn-failure message below: the closure takes ownership.
    let label = exe_stem.clone();
    let spawned = std::thread::Builder::new()
        .name("st-launch-raise".into())
        .spawn(move || {
            // COM for this thread's whole life rather than per poll: the
            // property-store reads inside `find_window_by_exe_stem` need it,
            // and this loop can call it 60+ times. Nested initialisation is a
            // refcount bump, so the inner guards become almost free. `None`
            // for every binding with no profile rule, which is most of them.
            let _com = unsafe { ComGuard::for_rule(&rule) };
            let t0 = std::time::Instant::now();
            let started_fg = unsafe { GetForegroundWindow() };
            std::thread::sleep(std::time::Duration::from_millis(SETTLE_MS));
            // The baseline no longer has to dodge the combo's own Space-up —
            // `last_user_typing_tick` excludes that by construction (PROBLEM
            // 221). It is still taken after the settle so the numbers in the
            // log line up with the first poll.
            let typing_baseline = crate::hook::last_user_typing_tick();

            let deadline = t0 + std::time::Duration::from_millis(GIVE_UP_MS);
            // Logged at most once per watcher: a foreground window we could not
            // identify is worth knowing about, but not 60 times.
            let mut said_unidentified = false;

            while std::time::Instant::now() < deadline {
                let since_launch = t0.elapsed().as_millis() as u64;

                // --- 1. Did a key go down? ---------------------------------
                let typed_tick = crate::hook::last_user_typing_tick();
                if typing_stand_down(since_launch, typing_baseline, typed_tick) {
                    let vk = crate::hook::last_user_typing_vk();
                    log::info!(
                        "raise_after_launch: '{exe_stem}' — STANDING DOWN. Observed: a key \
                         went DOWN (vk {vk:#04x}) about {since_launch} ms after the launch, \
                         past the {RAISE_GRACE_MS} ms grace, with Space not held. That is \
                         someone typing into something else, so the window will open \
                         wherever the app puts it. (Inside the grace this is ignored: the \
                         Space+key press IS the instruction to raise.)"
                    );
                    return;
                }

                // --- 2. Has the window we launched appeared? ---------------
                if let Some(hwnd) =
                    exe_stems.iter().find_map(|s| find_window_by_exe_stem(s, &rule))
                {
                    unsafe {
                        if IsIconic(hwnd).as_bool() {
                            log::info!(
                                "raise_after_launch: '{exe_stem}' opened MINIMIZED — restoring it"
                            );
                            let _ = ShowWindow(hwnd, SW_RESTORE);
                        }
                        if GetForegroundWindow() != hwnd {
                            log::info!(
                                "raise_after_launch: raising '{exe_stem}' {hwnd:?} \
                                 ({since_launch} ms after the launch)"
                            );
                            force_foreground(hwnd);
                        } else {
                            // COMMIT 0 (PROBLEM 225) — info!. "The app came up
                            // by itself" and "we had to force it" are the two
                            // outcomes any future report has to be able to
                            // tell apart, and one of them was invisible.
                            log::info!(
                                "raise_after_launch: '{exe_stem}' came up in front by itself \
                                 {since_launch} ms after the launch — nothing to do"
                            );
                        }
                    }
                    return;
                }

                // --- 3. Is some OTHER app's window in front? ---------------
                //
                // Not "did the foreground HWND change" — that fired for the
                // app's own splash screen and for the File Explorer window a
                // folder binding had just asked for. Identity, or nothing.
                let fg = unsafe { GetForegroundWindow() };
                let fg_changed = !fg.0.is_null() && fg != started_fg;
                // Only pay for the identity when it can change the answer.
                // Inside the grace the verdict is `KeepPolling` whatever this
                // says, and an `OpenProcess` + `QueryFullProcessImageNameW` on
                // every 120 ms tick for 8 s is ~63 syscall pairs for nothing.
                let fg_id = if fg_changed && since_launch >= RAISE_GRACE_MS {
                    window_process_identity(fg)
                } else {
                    None
                };
                match foreign_foreground_verdict(
                    since_launch,
                    fg_changed,
                    fg_id.as_ref().map(|(p, s)| (*p, s.as_str())),
                    launched_pid,
                    &exe_stems,
                ) {
                    ForeignVerdict::StandDown => {
                        let (fg_pid, fg_stem) = fg_id.unwrap_or((0, String::new()));
                        log::info!(
                            "raise_after_launch: '{exe_stem}' — STANDING DOWN. Observed: \
                             {fg_stem}.exe (pid {fg_pid}) has held the foreground since about \
                             {since_launch} ms after the launch, past the {RAISE_GRACE_MS} ms \
                             grace. It is neither this launch's process (pid \
                             {launched_pid}, 0 = the shell created none) nor any of its exe \
                             stems, so nothing here belongs to us. Nothing was raised."
                        );
                        return;
                    }
                    ForeignVerdict::Unidentified => {
                        if !said_unidentified {
                            said_unidentified = true;
                            log::info!(
                                "raise_after_launch: '{exe_stem}' — the foreground window \
                                 changed but its process could not be identified, so we are \
                                 NOT standing down: an unproven third window is no evidence \
                                 about what the user did. Still polling, and if '{exe_stem}' \
                                 appears before the deadline it WILL be raised over that \
                                 window; only if it never appears does the deadline expire \
                                 having raised nothing."
                            );
                        }
                    }
                    ForeignVerdict::KeepPolling => {}
                }

                std::thread::sleep(std::time::Duration::from_millis(POLL_MS));
            }
            log::warn!(
                "raise_after_launch: no window for '{exe_stem}' within {GIVE_UP_MS}ms \
                 (profile rule {rule:?}, launched pid {launched_pid}). Either it is slower \
                 than that, it has no top-level window of its own (installers and launcher \
                 stubs behave this way), or no window could PROVE it belongs to this \
                 binding's profile — in which case nothing was raised on purpose."
            );
        });

    if let Err(e) = spawned {
        // PROBLEM 124's rule: a spawn failure degrades the feature, never the app.
        log::warn!(
            "raise_after_launch: could not spawn the watcher ({e}) — '{label}' was still \
             launched, it just will not be pulled to the front."
        );
    }
}

#[cfg(not(windows))]
fn raise_after_launch(_exe_stems: Vec<String>, _rule: ProfileRule) {}

/// Find a top-level window belonging to a process with this exe stem.
///
/// FIND ONLY — deliberately not `try_focus_or_minimize`, which would MINIMISE
/// the window if it happened to be foreground already. Calling that from the
/// post-launch watcher would mean an app that opened correctly in front got
/// hidden again a moment later: a worse bug than the one being fixed. It also
/// does not touch the cascade's HWND cache, which is the cascade's own
/// bookkeeping and not this thread's business.
#[cfg(windows)]
fn find_window_by_exe_stem(exe_stem: &str, rule: &ProfileRule) -> Option<HWND> {
    let _com = unsafe { ComGuard::for_rule(rule) };
    let mut search = StemSearch {
        stem: exe_stem.to_lowercase(),
        rule: rule.clone(),
        found: None,
        declined: Vec::new(),
    };
    unsafe {
        // EnumWindows is synchronous, so a pointer to this stack value cannot
        // outlive the call and there is nothing to reclaim afterwards. (The
        // old shape boxed the payload; an early version of it forgot the
        // matching `from_raw` and leaked on every uncached press.)
        let _ = EnumWindows(
            Some(enum_callback),
            windows::Win32::Foundation::LPARAM(&mut search as *mut StemSearch as isize),
        );
    }
    search.found
}

/// The exe stem a launch target will show up as in the window list.
/// `C:\Apps\Brave\brave.exe` → `brave`; `Discord.lnk` → `discord`.
fn launch_stem(target: &str) -> String {
    std::path::Path::new(target)
        .file_stem()
        .map(|f| f.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

/// Four-step foreground ladder — the minimum set that reliably beats
/// Windows 10/11's focus-theft prevention (adapted from MSDN community
/// notes, AutoHotkey source, and the implementation plan §Step 11).
///
/// Steps:
///  1. AttachThreadInput to BOTH the outgoing foreground thread and the
///     TARGET's thread, so all three share one input queue and
///     SetForegroundWindow is not blocked by UIPI / focus-lock.
///  2. BringWindowToTop + SetForegroundWindow — first actual raise.
///  3. Synthetic app-inert keydown+up (VK_NONAME, cookie 0x7A7A7A7A) to nudge
///     the foreground lock.
///  4. SwitchToThisWindow — final reliable hand-off, and the one that can
///     flash the taskbar button.
///
/// After every step: verify with GetForegroundWindow() and log which step
/// actually brought the window to front, so we have data if a step regresses.
///
/// TWO CORRECTIONS, PROBLEM 225.
///
/// **Step 1 was half-implemented.** It attached this thread to the OUTGOING
/// foreground thread only. The documented recipe — the one AutoHotkey and every
/// other launcher uses — attaches the caller to the outgoing foreground thread
/// AND to the target window's thread, so all three share one input queue across
/// the `BringWindowToTop` + `SetForegroundWindow` pair. There was no
/// `GetWindowThreadProcessId(hwnd, ...)` anywhere in this function. That missing
/// attach is the most likely reason for the single measured denial in the
/// owner's log (`all 4 steps failed`, brave, 2026-08-31 08:55:54).
///
/// **Step 3's justification was wrong, and its key was dangerous.** The comment
/// claimed the tap makes Windows treat US as the last input recipient. It does
/// not: injected input is delivered to the CURRENT FOREGROUND window's queue, so
/// it made the OLD app the recipient. What the tap historically did was cancel
/// menus and nudge the foreground lock — folklore, and unreliable on Win11, but
/// harmless. `VK_MENU` was not harmless: an Alt down+up delivered to the current
/// foreground app opens its menu bar / KeyTips, and the owner's launch targets
/// are Word, File Explorer, Chrome and Brave — every one of which reacts to a
/// bare Alt. It is now `VK_NONAME` (0xFC), which is reserved and does nothing in
/// any application. The `0x7A7A7A7A` cookie stays (our own hook must ignore it,
/// and `hook::LAST_USER_TYPING` excludes it from the typing signal by the same
/// cookie); it is NOT filtered by `LLKHF_INJECTED`, which stays banned.
#[cfg(windows)]
unsafe fn force_foreground(hwnd: windows::Win32::Foundation::HWND) {
    let fg_before = GetForegroundWindow();
    if fg_before == hwnd {
        return; // Already foreground — nothing to do
    }
    let fg_thread = GetWindowThreadProcessId(fg_before, None);
    let my_thread = GetCurrentThreadId();

    // Step 1 — attach to the current foreground thread so SetForegroundWindow
    // is not blocked by Windows' focus-lock. Detach immediately after.
    //
    // PROBLEM 121 — NEVER attach to a thread that is already wedged.
    //
    // While two threads are attached they SHARE one input queue. That is the
    // whole point (it is what defeats the focus lock) and also the whole
    // danger: if the other side is not pumping messages, our call into it
    // blocks, and its input processing stalls along with ours. Two
    // applications go unresponsive instead of one.
    //
    // The exposure here is not theoretical. This path runs on every focus and
    // every restore — 100+ times in a single day on this owner's machine,
    // almost all of them Brave and Discord, which are exactly the two
    // applications he reported "stop responding". That is a correlation, not
    // a proof, but attaching to a hung thread has no upside worth defending.
    //
    // `IsHungAppWindow` asks Windows precisely the right question: is this
    // window's thread failing to pump its message queue? If it is, skip the
    // attach. The only cost is that SetForegroundWindow may lose the focus
    // race against a Windows lock — a shortcut that does not raise a window,
    // versus a shortcut that can freeze the window it was aimed at.
    // PROBLEM 133 - the TARGET must be checked too, and it is the one that
    // matters for the reported symptom.
    //
    // PROBLEM 121 (above) guards the window we are switching AWAY from,
    // because that is whose input queue AttachThreadInput joins. Correct, but
    // incomplete: `BringWindowToTop` and `SetForegroundWindow` below are aimed
    // at `hwnd`, the TARGET, and nothing asked whether IT was alive. A call
    // into a wedged window's thread blocks the caller - and we may be attached
    // to a second thread at the time, so the stall spreads to a third party.
    //
    // The evidence that this was the real gap: `fg_before` is normally the
    // healthy window the owner is looking at, while the sick one is whatever
    // he just aimed a shortcut at. So the guard sat on the wrong window and
    // NEVER FIRED ONCE in the entire log - 100+ focus/restore operations,
    // almost all Brave and Discord, the exact two apps reported as freezing.
    // A protection that cannot fire is indistinguishable from no protection,
    // and it reads as covered in a code review.
    //
    // IsHungAppWindow only reports true after ~5s of a thread not pumping, so
    // this cannot trigger on a merely busy app. If it says hung, raising the
    // window was never going to work anyway; the only question was whether we
    // hung too.
    let target_hung = IsHungAppWindow(hwnd).as_bool();
    if target_hung {
        log::warn!(
            "force_foreground: the TARGET window is not responding - not touching it              (PROBLEM 133). Raising a wedged window cannot succeed and risks dragging              Spaceadom down with it. The app is stuck on its own; this shortcut is a              no-op until it recovers."
        );
        return;
    }

    let fg_hung = IsHungAppWindow(fg_before).as_bool();
    if fg_hung {
        log::warn!(
            "force_foreground: the current foreground window is not responding — \
             skipping AttachThreadInput so we are not dragged down with it \
             (PROBLEM 121). Focus may not switch this time."
        );
    }
    // PROBLEM 225 — the TARGET's thread, the half of the recipe that was
    // missing. Read here and not at the top of the function on purpose: the
    // `IsHungAppWindow(hwnd)` early-return above MUST stay above every line
    // that touches the target's input queue, and keeping this below it makes
    // that ordering impossible to break by accident. Do not move either one.
    let target_thread = GetWindowThreadProcessId(hwnd, None);

    // PROBLEM 227 (a) — "the new attach inherits that guard for free" was not
    // true where the attach was added FOR.
    //
    // `IsHungAppWindow` only returns true after ~5 s of a thread not pumping.
    // The target this attach was aimed at is `raise_after_launch`'s, and that
    // window is younger than `GIVE_UP_MS` (8 s) and usually younger than 2 s —
    // so on the launch path the hang detector's threshold is longer than the
    // window's entire life and the early return above CANNOT produce a
    // negative. A check that cannot produce a negative result is not a check
    // (CLAUDE.md testing laws), and it read as covered in review.
    //
    // So ask a question a 600 ms old window CAN fail: send it `WM_NULL` with a
    // hard timeout. A thread that is pumping answers a null message in
    // microseconds; a thread that is mid-startup and not yet pumping does not
    // answer at all, and we find out in `PUMP_PROBE_MS` instead of by blocking
    // inside `SetForegroundWindow` with our input queue already joined to it.
    //
    // Bounded on purpose: this runs on the ENGINE thread from
    // `try_focus_or_minimize`, so an unbounded wait here stalls every shortcut.
    // `SMTO_ABORTIFHUNG` returns immediately for the ≥5 s case the early return
    // already covers, and the timeout covers the case it cannot see.
    //
    // Failure only skips the ATTACH, never the raise: `BringWindowToTop` /
    // `SetForegroundWindow` are still called, and steps 3 and 4 still run. The
    // cost of being wrong is a raise that has to fall through to
    // `SwitchToThisWindow`; the cost of the other choice is PROBLEM 121's two
    // frozen applications.
    const PUMP_PROBE_MS: u32 = 100;
    let target_pumping = target_thread == my_thread || {
        use windows::Win32::Foundation::{LPARAM, WPARAM};
        use windows::Win32::UI::WindowsAndMessaging::{
            SendMessageTimeoutW, SMTO_ABORTIFHUNG, WM_NULL,
        };
        SendMessageTimeoutW(
            hwnd,
            WM_NULL,
            WPARAM(0),
            LPARAM(0),
            SMTO_ABORTIFHUNG,
            PUMP_PROBE_MS,
            None,
        )
        .0 != 0
    };
    if !target_pumping {
        log::info!(
            "force_foreground: the target window did not answer a WM_NULL within \
             {PUMP_PROBE_MS}ms — it is not pumping its message queue yet (an app still \
             starting up reads exactly like this, and IsHungAppWindow cannot see it for \
             ~5s). NOT attaching our input queue to it (PROBLEM 227); the raise still \
             goes ahead without the attach."
        );
    }

    // TWO INDEPENDENT FLAGS, and PROBLEM 121's whole cost was a leaked
    // attachment, so neither expression may ever be re-derived at detach time.
    // Each `AttachThreadInput(.., true)` below is undone on exactly the bool
    // that authorised it.
    let attached_fg = fg_thread != my_thread && fg_thread != 0 && !fg_hung;
    // Skip when it is the same queue we are already on or already attached to:
    // attaching a thread to itself fails, and attaching twice to one thread
    // would need two detaches to unwind.
    //
    // PROBLEM 227 (b) — `!fg_hung` belongs here too, and its absence was an
    // asymmetry with a concrete failure. Brave is wedged and holds the
    // foreground (the owner's reported pair); Space+D is pressed for a healthy
    // Discord. `IsHungAppWindow(discord_hwnd)` is false, so no early return;
    // `fg_hung` is true, so we correctly skip the fg attach and log "Focus may
    // not switch this time" — and then attached to DISCORD's UI thread anyway
    // and called `BringWindowToTop`/`SetForegroundWindow`, whose activation
    // work still has to reach the wedged outgoing window. If that blocks, the
    // caller stalls WHILE SHARING AN INPUT QUEUE with Discord, so the healthy
    // app is dragged down by the raise aimed at it. From `try_focus_or_minimize`
    // the caller is the ENGINE thread, so every shortcut dies with it.
    //
    // The attach only ever existed to defeat the foreground LOCK, and defeating
    // the lock requires the FOREGROUND thread's queue. Once that half is
    // skipped, attaching to the target alone buys nothing and keeps all of the
    // risk — so when the outgoing window is wedged we attach to neither, which
    // is exactly what the log line above already promises the reader.
    let attached_target = target_thread != 0
        && target_thread != my_thread
        && target_thread != fg_thread
        && !fg_hung
        && target_pumping;

    if attached_fg {
        let _ = AttachThreadInput(my_thread, fg_thread, true);
    }
    if attached_target {
        let _ = AttachThreadInput(my_thread, target_thread, true);
    }

    // Step 2 — bring to top + set foreground (may succeed on its own for
    // windows that allow being foregrounded).
    let _ = BringWindowToTop(hwnd);
    let _ = SetForegroundWindow(hwnd);

    // Detach on exactly the condition we attached on. Deriving it a second
    // time from the same operands would leave a permanent attachment if any
    // of them changed in between — and a leaked attachment is the same freeze,
    // with no way back short of restarting the app. Reverse order of the
    // attaches, so the queues unwind the way they were built.
    if attached_target {
        let _ = AttachThreadInput(my_thread, target_thread, false);
    }
    if attached_fg {
        let _ = AttachThreadInput(my_thread, fg_thread, false);
    }

    if GetForegroundWindow() == hwnd {
        log::info!(
            "force_foreground: step-2 (attach + BringWindowToTop + SetForegroundWindow) \
             succeeded — clean raise, no taskbar flash"
        );
        return;
    }

    // Step 3 — a synthetic, APP-INERT key tap.
    //
    // WHAT THIS IS NOT (PROBLEM 225). The old comment here said Windows
    // "requires the last input to be a keyboard event before it honours
    // SetForegroundWindow from a thread that didn't initiate user input", and
    // that this tap satisfies it. It does not, and cannot: `SendInput`
    // delivers to the CURRENT FOREGROUND window's input queue, so the app we
    // are trying to switch AWAY from becomes the last-input recipient, not us.
    // The documented carve-out was never met by this call.
    //
    // WHAT IT ACTUALLY DOES: cancels any open menu and nudges the foreground
    // lock. That is folklore, and unreliable on Win11, but it is free and it is
    // ahead of the step that flashes the taskbar — so it stays, honestly
    // described.
    //
    // WHY NOT VK_MENU. It used to send Alt down+up, which is NOT benign: a bare
    // Alt delivered to the foreground app opens its menu bar or KeyTips, and
    // the owner's launch targets are Word, File Explorer, Chrome and Brave —
    // all four react to it. VK_NONAME (0xFC) is reserved by Windows, is
    // documented as having no effect, and no application binds it.
    //
    // The 0x7A7A7A7A cookie is load-bearing twice over: our own low-level hook
    // ignores it (never `LLKHF_INJECTED` — see NATIVE_SAFETY row 4), and
    // `hook::LAST_USER_TYPING` excludes it by the same test, so this tap can
    // never stand the post-launch watcher down against itself. Both keys go in
    // ONE `SendInput` batch — order is only guaranteed when we own the batch.
    const VK_NONAME: u16 = 0xFC;
    let inputs: [INPUT; 2] = [
        INPUT {
            r#type: INPUT_TYPE(1), // INPUT_KEYBOARD
            Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(VK_NONAME),
                    wScan: 0,
                    dwFlags: KEYBD_EVENT_FLAGS(0),
                    time: 0,
                    dwExtraInfo: 0x7A7A7A7A,
                },
            },
        },
        INPUT {
            r#type: INPUT_TYPE(1),
            Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(VK_NONAME),
                    wScan: 0,
                    dwFlags: KEYBD_EVENT_FLAGS(KEYEVENTF_KEYUP.0),
                    time: 0,
                    dwExtraInfo: 0x7A7A7A7A,
                },
            },
        },
    ];
    // NEVER LEAVE A MODIFIER DOWN: the pair is symmetric and in one batch, so
    // there is no window in which a key is held. That rule is why this is a
    // reserved vk and not, say, Shift.
    let sent = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    // COMMIT 0 (PROBLEM 225) applied to the line it missed. Every other step
    // outcome in this function was promoted from `debug!` to `info!` because
    // release builds filter at Info (logger.rs) and "step 2 succeeded" was
    // indistinguishable from "step 4 flashed the taskbar" in every report the
    // owner has ever sent. This line — the one that says whether the injection
    // happened at all — stayed at debug, which left `sent == 0` (SendInput
    // blocked by UIPI, or by another thread's `BlockInput`) as the last way
    // step 3 can silently do nothing. WARN when the batch was cut short,
    // because a partial insert is the interesting case.
    if sent as usize != inputs.len() {
        log::warn!(
            "force_foreground: SendInput inserted {sent} of {} events (VK_NONAME, \
             cookie-tagged). 0 means the injection was blocked outright — UIPI (an \
             elevated window has focus) or another thread's BlockInput. Step 3 did \
             nothing this time; the raise falls through to step 4.",
            inputs.len()
        );
    } else {
        log::info!(
            "force_foreground: SendInput sent {sent} events (VK_NONAME, cookie-tagged)"
        );
    }

    let _ = SetForegroundWindow(hwnd);
    if GetForegroundWindow() == hwnd {
        log::info!(
            "force_foreground: step-3 (cookie-tagged VK_NONAME tap + SetForegroundWindow) \
             succeeded — clean raise, no taskbar flash"
        );
        return;
    }

    // Step 4 — SwitchToThisWindow: last-resort, always works but may flash
    // the taskbar button once.
    SwitchToThisWindow(hwnd, true);
    if GetForegroundWindow() == hwnd {
        log::info!(
            "force_foreground: step-4 SwitchToThisWindow succeeded — THE WINDOW IS UP BUT ITS \
             TASKBAR BUTTON MAY HAVE FLASHED. If the owner reports a flash, this is the line \
             that proves it: steps 2 and 3 were both denied and only the last resort worked."
        );
    } else {
        log::warn!("force_foreground: all 4 steps failed for HWND {:?}", hwnd);
    }
}

/// Step 12 — AUMID-based focus/minimize for Microsoft Store apps.
///
/// Store apps' windows belong to `ApplicationFrameHost.exe`, not the app's own
/// process. `QueryFullProcessImageNameW` returns the HOST process, so the
/// normal exe-stem matching never finds them. Instead we enumerate all
/// top-level windows and call `SHGetPropertyStoreForWindow` to read
/// `PKEY_AppUserModel_ID`, which IS the user's `shell:AppsFolder\<AUMID>`
/// string (without the `shell:AppsFolder\` prefix).
///
/// If a window with a matching AUMID is found: toggle focus/minimize.
/// Returns `true` if acted, `false` if no matching window found (caller
/// should then launch the app via `shell_launch`).
#[cfg(windows)]
/// Site keyword for matching a browser window title, derived from a URL.
///
/// `https://www.youtube.com/watch?v=…` → `Some(("youtube", "youtube.com"))`
///
/// Returns `None` when the keyword would be too weak to match on safely — a
/// 1-2 character first label (`x.com`, `t.co`) would match almost any title
/// and could minimise an unrelated browser window. Callers treat `None` as
/// "just launch the URL", which is always safe.
fn url_match_keys(url: &str) -> Option<(String, String)> {
    // Strip scheme, then path/query/fragment, then userinfo and port.
    let rest = url
        .split_once("://")
        .map(|(_, r)| r)
        .unwrap_or(url);
    let hostport = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or(rest);
    let host = hostport
        .rsplit('@')
        .next()
        .unwrap_or(hostport)
        .split(':')
        .next()
        .unwrap_or(hostport)
        .trim_start_matches("www.")
        .to_lowercase();
    if host.is_empty() {
        return None;
    }
    let keyword = host.split('.').next().unwrap_or(&host).to_string();
    if keyword.len() < 3 {
        return None;
    }
    Some((keyword, host))
}

/// The package family name — everything before the `!` in an AUMID.
/// `SamsungElectronicsCo.Ltd.PCGallery_3c1yjt4zspk6g!App` → the part before
/// `!`. Stable per package; the app-id after `!` is not.
fn aumid_family(s: &str) -> &str {
    s.split('!').next().unwrap_or(s)
}

/// PROBLEM 79 — is this window DWM-cloaked? Suspended packaged apps and
/// windows parked on another virtual desktop stay IsWindowVisible()==true but
/// composite nothing; restoring one moves keyboard FOCUS to a window the user
/// cannot see (Enter could send a WhatsApp message into the void). Every
/// window-matching enum must skip cloaked windows.
#[cfg(windows)]
unsafe fn is_cloaked(hwnd: windows::Win32::Foundation::HWND) -> bool {
    use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
    let mut cloaked: u32 = 0;
    let _ = DwmGetWindowAttribute(
        hwnd,
        DWMWA_CLOAKED,
        &mut cloaked as *mut _ as *mut core::ffi::c_void,
        4,
    );
    cloaked != 0
}

/// PROBLEM 79, fallback 2 — package family name of the process that owns
/// `hwnd`, lowercased. None for unpackaged processes (kernel32 returns
/// APPMODEL_ERROR_NO_PACKAGE) and for processes we cannot open (protected —
/// skip, never abort the enumeration).
#[cfg(windows)]
unsafe fn window_package_family(hwnd: windows::Win32::Foundation::HWND) -> Option<String> {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::{CloseHandle, ERROR_SUCCESS};
    use windows::Win32::Storage::Packaging::Appx::GetPackageFamilyName;
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;

    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if pid == 0 || pid == std::process::id() {
        return None;
    }
    let hproc = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
    // PACKAGE_FAMILY_NAME_MAX_LENGTH is 64; +1 for the NUL.
    let mut buf = [0u16; 65];
    let mut len = buf.len() as u32;
    let err = GetPackageFamilyName(hproc, &mut len, PWSTR(buf.as_mut_ptr()));
    let _ = CloseHandle(hproc);
    if err != ERROR_SUCCESS {
        return None; // unpackaged (15700) or query failure — either way, skip
    }
    // On success `len` INCLUDES the NUL terminator.
    Some(String::from_utf16_lossy(&buf[..(len as usize).saturating_sub(1)]).to_lowercase())
}

/// PROBLEM 79, fallback 3 — the target exe path of an Apps-folder entry
/// (e.g. Arc: "C:\...\Arc\Arc.exe"). Packaged apps have no
/// System.Link.TargetParsingPath — GetString errs cleanly and we return None.
/// MUST run while COM is initialised on this thread. Takes the ORIGINAL-case
/// shell target: registered AUMIDs are arbitrary case-sensitive strings, and
/// parsing a lowercased copy can fail with 0x80070002.
#[cfg(windows)]
unsafe fn apps_folder_target_path(shell_target: &str) -> Option<String> {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::System::Com::{CoTaskMemFree, IBindCtx};
    use windows::Win32::UI::Shell::{IShellItem2, SHCreateItemFromParsingName};
    use windows::Win32::UI::Shell::PropertiesSystem::PROPERTYKEY;
    use windows::core::GUID;

    // System.Link.TargetParsingPath — hand-rolled like PKEY_AppUserModel_ID
    // below, so no new Cargo feature (PROBLEM 30 class) is needed.
    let pkey_target = PROPERTYKEY {
        fmtid: GUID::from_u128(0xB9B4B3FC_2B51_4A42_B5D8_324146AFCF25),
        pid: 2,
    };

    let name = HSTRING::from(shell_target);
    let item: IShellItem2 =
        SHCreateItemFromParsingName(PCWSTR(name.as_ptr()), None::<&IBindCtx>).ok()?;
    let pwstr = item.GetString(&pkey_target).ok()?;
    let path = pwstr.to_string().ok();
    CoTaskMemFree(Some(pwstr.0 as *const _));
    path
}

fn aumid_focus_or_minimize(shell_target: &str, rule: &ProfileRule) -> bool {
    // Strip the "shell:AppsFolder\" prefix to get the bare AUMID.
    // NOTE: `shell_target` itself must stay ORIGINAL-case — fallback 3 parses
    // it through the shell namespace, which is case-sensitive for registered
    // AUMIDs. Only this comparison copy is lowercased.
    let aumid = shell_target
        .strip_prefix("shell:AppsFolder\\")
        .or_else(|| shell_target.strip_prefix("shell:appsFolder\\"))
        .unwrap_or(shell_target)
        .to_lowercase();

    use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, IsWindowVisible};
    use windows::Win32::System::Com::{
        CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Shell::PropertiesSystem::{
        SHGetPropertyStoreForWindow, IPropertyStore, GPS_READWRITE,
    };
    use windows::core::PCWSTR;
    // PKEY_AppUserModel_ID: {9F4C2855-9F79-4B39-A8D0-E1D42DE1D5F3}, 5
    use windows::Win32::UI::Shell::PropertiesSystem::PROPERTYKEY;
    use windows::core::GUID;

    let pkey_aumid = PROPERTYKEY {
        fmtid: GUID {
            data1: 0x9F4C2855,
            data2: 0x9F79,
            data3: 0x4B39,
            data4: [0xA8, 0xD0, 0xE1, 0xD4, 0x2D, 0xE1, 0xD5, 0xF3],
        },
        pid: 5,
    };

    struct SearchPayload {
        aumid: String,
        found: Option<HWND>,
        pkey: PROPERTYKEY,
        /// Every packaged window's AUMID we looked at, for diagnostics when
        /// nothing matches.
        seen: Vec<String>,
    }

    unsafe extern "system" fn aumid_enum_cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
        if !IsWindowVisible(hwnd).as_bool() {
            return BOOL(1);
        }
        // PROBLEM 79 — a cloaked ApplicationFrameWindow keeps its AUMID
        // property; matching one restores focus onto an invisible window.
        if is_cloaked(hwnd) {
            return BOOL(1);
        }
        let payload = &mut *(lparam.0 as *mut SearchPayload);

        // ONE property-store reader in this app. `window_string_property` owns
        // the SHGetPropertyStoreForWindow / GetValue / PropVariantToStringAlloc
        // / CoTaskMemFree sequence that used to be written out here, and the
        // browser-profile matcher added in 2026-08-27 calls the same function
        // rather than repeating it. A failure at any step still means "skip
        // this window", never "abort the enumeration".
        let Some(window_aumid) = window_string_property(hwnd, &payload.pkey) else {
            return BOOL(1);
        };
        let window_aumid = window_aumid.to_lowercase();

        // Exact match first, then PACKAGE FAMILY NAME (the part before
        // '!'). Windows does NOT guarantee a packaged app's window
        // reports the same AUMID that launched it — the app-id after
        // '!' is chosen by the app, and apps with several entry points
        // launch as "…!App" while their window reports "…!Gallery" or
        // similar. Exact-only matching is why Samsung Notes minimised
        // correctly on 2026-08-11 while Samsung Gallery relaunched
        // every time: same code, different app. The family name before
        // '!' identifies the package uniquely, so it is a safe
        // fallback — it cannot collide across different packages.
        let matched = window_aumid == payload.aumid
            || (aumid_family(&window_aumid) == aumid_family(&payload.aumid)
                && !aumid_family(&payload.aumid).is_empty());

        if matched {
            payload.found = Some(hwnd);
            return BOOL(0); // stop enumeration
        }
        // Record what packaged windows we DID see. Without this, "no
        // match" is indistinguishable from "no packaged windows at
        // all", and the only way to tell was to add logging and ask
        // the user to reproduce — a whole round trip.
        if !window_aumid.is_empty() {
            payload.seen.push(window_aumid);
        }
        BOOL(1)
    }

    let mut payload = SearchPayload {
        aumid,
        found: None,
        pkey: pkey_aumid,
        seen: Vec::new(),
    };

    unsafe {
        // COM must be initialised on this thread for SHGetPropertyStoreForWindow
        // AND for fallback 3's SHCreateItemFromParsingName. The CoUninitialize
        // moved BELOW the whole ladder (PROBLEM 79) — uninitialising here made
        // the shell parse fail with CO_E_NOTINITIALIZED, an Err that reads
        // exactly like "property not found" and silently disables the fix.
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

        let _ = EnumWindows(
            Some(aumid_enum_cb),
            LPARAM(&mut payload as *mut SearchPayload as isize),
        );

        // ------------------------------------------------------------------
        // PROBLEM 79, fallback 2 — match by the PROCESS's package family.
        // WinUI3 apps (modern WhatsApp: process WhatsApp.Root) own visible,
        // titled windows that carry NO AppUserModel_ID property, so the
        // property-store pass above never sees them — every press fell
        // through to ShellExecute re-activation, which no-ops on a running
        // instance ("it does nothing", tester + owner, 2026-08-12).
        // Only meaningful for PACKAGED bindings — a family exists only when
        // the AUMID has the "family!appid" shape.
        // ------------------------------------------------------------------
        if payload.found.is_none() && payload.aumid.contains('!') {
            use windows::Win32::UI::WindowsAndMessaging::{
                GetWindow, GetWindowTextLengthW, GW_OWNER,
            };

            struct FamilyScan {
                family: String,
                candidates: Vec<HWND>,
            }
            unsafe extern "system" fn family_enum_cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
                let scan = &mut *(lparam.0 as *mut FamilyScan);
                // Cheap rejections first: visible → not cloaked → titled →
                // unowned. Titleless kills XAML flyout popups; GW_OWNER kills
                // owned dialogs so a modal's MAIN window is what we act on.
                if !IsWindowVisible(hwnd).as_bool() || is_cloaked(hwnd) {
                    return BOOL(1);
                }
                if GetWindowTextLengthW(hwnd) == 0 {
                    return BOOL(1);
                }
                if GetWindow(hwnd, GW_OWNER).map(|h| !h.is_invalid()).unwrap_or(false) {
                    return BOOL(1);
                }
                if window_package_family(hwnd).as_deref() == Some(scan.family.as_str()) {
                    // NO early exit — collect, then rank. A topmost helper
                    // (mini player, toast) enumerates before the main window;
                    // first-hit would toggle the helper forever.
                    scan.candidates.push(hwnd);
                }
                BOOL(1)
            }

            let mut scan = FamilyScan {
                family: aumid_family(&payload.aumid).to_string(),
                candidates: Vec::new(),
            };
            let _ = EnumWindows(
                Some(family_enum_cb),
                LPARAM(&mut scan as *mut FamilyScan as isize),
            );
            // Rank: the foreground window if it matched (so the minimize half
            // of the toggle acts on what the user is looking at), else the
            // first in enum order (≈ most recently active).
            let fg = GetForegroundWindow();
            payload.found = scan
                .candidates
                .iter()
                .copied()
                .find(|&h| h == fg)
                .or_else(|| scan.candidates.first().copied());
            if let Some(h) = payload.found {
                log::info!(
                    "aumid_focus: matched by PROCESS package family {:?} ({} candidate(s)) — {:?}",
                    scan.family,
                    scan.candidates.len(),
                    h
                );
            }
        }

        // ------------------------------------------------------------------
        // PROBLEM 79, fallback 3 — unpackaged Apps-folder entries (Arc et
        // al.): the entry is shortcut-backed, so resolve its target exe and
        // delegate to the Win32 stem matcher, which owns the same
        // minimize/restore cycle plus the HWND cache.
        // ------------------------------------------------------------------
        if payload.found.is_none() {
            if let Some(target) = apps_folder_target_path(shell_target) {
                log::info!(
                    "aumid_focus: Apps-folder entry resolves to {:?} — delegating to the stem matcher",
                    target
                );
                CoUninitialize();
                // The profile rule travels with the delegation. An unpackaged
                // Apps-folder entry can perfectly well BE a Chromium browser —
                // Arc is exactly that on this machine — so dropping the rule
                // here would leave one route into the stem matcher still
                // profile-blind.
                return try_focus_or_minimize(&target, rule);
            }
        }

        CoUninitialize();
    }

    if let Some(hwnd) = payload.found {
        unsafe {
            let is_active = GetForegroundWindow() == hwnd;
            let is_minimized = IsIconic(hwnd).as_bool();
            if is_active && !is_minimized {
                log::info!("aumid_focus: AUMID match — minimizing HWND {:?}", hwnd);
                let _ = ShowWindow(hwnd, SW_MINIMIZE);
            } else {
                log::info!("aumid_focus: AUMID match — restoring HWND {:?}", hwnd);
                let _ = ShowWindow(hwnd, SW_RESTORE);
                force_foreground(hwnd);
            }
        }
        true
    } else {
        // log::info, not debug — debug lines are filtered out of the shipped
        // log, so the ONE line that explains why a Store app relaunched
        // instead of minimising was invisible exactly when it was needed.
        // Listing what we DID see turns "it doesn't work" into a one-look fix.
        log::info!(
            "aumid_focus: no window matched AUMID {:?} (family {:?}). Packaged windows seen: {:?}",
            shell_target,
            aumid_family(&payload.aumid),
            payload.seen,
        );
        false
    }
}

#[cfg(not(windows))]
fn aumid_focus_or_minimize(_shell_target: &str, _rule: &ProfileRule) -> bool { false }

/// Absolute path to the user's DEFAULT browser executable.
///
/// PROBLEM 60. This used to guess brave → chrome → msedge, which meant the
/// URL toggle (`url_focus_or_minimize`) looked for windows belonging to a
/// browser the user may not even have — so Space+Y could never find the tab it
/// had just opened, and every press launched a duplicate.
///
/// Resolved properly from the shell association Windows itself uses:
///   HKCU\...\UrlAssociations\https\UserChoice → ProgId
///   HKCR\<ProgId>\shell\open\command          → "C:\...\firefox.exe" -- "%1"
///
/// SPLIT OUT OF `browser_stem` 2026-08-26 (TASK 3): the stem was all this
/// module ever needed, but the key editor's paste row now draws the OS default
/// browser's real icon, and an icon needs the PATH. One resolver, two readers —
/// the alternative was a second registry walk that could disagree with this one
/// about which browser is the default, which is the whole class of bug
/// PROBLEM 60 was.
#[cfg(windows)]
pub fn default_browser_exe() -> Option<String> {
    use winreg::enums::{HKEY_CLASSES_ROOT, HKEY_CURRENT_USER};
    use winreg::RegKey;

    let prog_id: String = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(
            r"SOFTWARE\Microsoft\Windows\Shell\Associations\UrlAssociations\https\UserChoice",
        )
        .ok()?
        .get_value("ProgId")
        .ok()?;

    let cmd: String = RegKey::predef(HKEY_CLASSES_ROOT)
        .open_subkey(format!(r"{prog_id}\shell\open\command"))
        .ok()?
        .get_value("")
        .ok()?;

    // cmd looks like: "C:\Program Files\Mozilla Firefox\firefox.exe" -osint -url "%1"
    // Take the quoted path if present, else everything up to the first space.
    let path = if cmd.starts_with('"') {
        cmd[1..].split('"').next()?.to_string()
    } else {
        cmd.split_whitespace().next()?.to_string()
    };

    let stem = std::path::Path::new(&path)
        .file_stem()
        .map(|s| s.to_string_lossy().to_lowercase())?;
    // Marker text unchanged on purpose — "default browser resolved" is a
    // findable line in the log and an ASCII marker in the exe; the path is
    // appended, never substituted.
    log::info!("cascade: default browser resolved → {stem} (ProgId {prog_id}) at {path}");
    Some(path)
}

#[cfg(not(windows))]
pub fn default_browser_exe() -> Option<String> {
    None
}

/// Which browser `run_browser` would use, as a lowercase process stem
/// ("firefox", "msedge"). Must stay in step with run_browser's preference
/// order, which it does by construction — both go through
/// `default_browser_exe`.
#[cfg(windows)]
fn browser_stem() -> Option<String> {
    let path = default_browser_exe()?;
    std::path::Path::new(&path)
        .file_stem()
        .map(|s| s.to_string_lossy().to_lowercase())
}

#[cfg(not(windows))]
fn browser_stem() -> Option<String> {
    None
}

/// URL bindings get the same launch → focus → minimise cascade as apps.
///
/// WHY THIS EXISTS (2026-08-11): `run_browser` unconditionally launched the
/// URL, so pressing Space+Y a second time opened ANOTHER YouTube tab instead
/// of toggling. URL bindings were the only kind with no cascade at all.
///
/// Returns true if it handled the press (focused or minimised an existing
/// window); false means "no window is showing this site" and the caller
/// should launch the URL normally.
///
/// DELIBERATELY NOT SENDING Ctrl+W. The user proposed closing the tab on the
/// second press. Ctrl+W closes whatever tab is ACTIVE, which is not
/// necessarily the bound site — a half-written comment or form would be
/// destroyed, on a key pressed dozens of times a day, and the app cannot
/// check-then-send without racing the user. Minimising achieves the actual
/// goal ("get it out of my way") with no destruction. Do not "improve" this
/// into a tab-closing feature.
///
/// KNOWN LIMITATION: a window title only reveals its ACTIVE tab. If the site
/// sits in a background tab we return false and open a duplicate. Detecting
/// background tabs needs browser-extension access — out of scope. A duplicate
/// tab is a far smaller harm than closing the wrong one.
/// TASK 4, 2026-08-27 — A SECOND, INDEPENDENT DEFECT ON THE SAME FEATURE.
///
/// This function used to take only the URL and resolve the browser purely from
/// `default_browser_exe()`, ignoring the binding's OWN `browser_exe` entirely.
/// On this machine the default browser is Brave, so a URL pinned to CHROME
/// Profile 6 went hunting through BRAVE's windows. It could never find its own
/// tab, so every press opened a duplicate — precisely the symptom this function
/// was written to prevent, reintroduced through the back door by the pinned-
/// browser feature.
///
/// THE HARD REQUIREMENT IS UNTOUCHED. The owner has stated it twice: a URL with
/// no specific browser set MUST open in the OS default browser. The decision is
/// made by `route_for`, the same guarded function the LAUNCH leg asks, so the
/// window this looks for and the window `open_binding_url` would open cannot
/// disagree — and a binding with no `browser_exe` returns
/// `BrowserRoute::Default` without so much as a filesystem call and lands on
/// `browser_stem()`, exactly as it always has.
///
/// THE URL LEG'S RULE, IN ONE SENTENCE (rewritten 2026-08-27 after review, and
/// the reason it takes no `claims`): **the profile this looks for is exactly
/// the `--profile-directory` the launch leg is about to pass, and `Any` when
/// the launch will pass none.**
///
/// The first draft of this instead applied `claims_rule` on every arm, and
/// three reviewers were right to call it a regression. Two of these three arms
/// launch through `run_browser`, which passes NO profile switch and deliberately
/// raises with `ProfileRule::Any` (see its comment at the `raise_after_launch`
/// call) — so a claim could make this leg DECLINE the very window the launch
/// would land in, and then the match and the launch could never agree. The
/// owner's machine is the worst case for it: Brave is the default browser AND
/// the browser he pins profiles of, so a plain `youtube.com` key that pins
/// nothing at all would have declined his YouTube window, opened a duplicate
/// tab, and done it again on every press — forever, with no minimise half.
/// That is the exact bug this function was written on 2026-08-11 to prevent.
///
/// WHY DROPPING CLAIMS IS SAFE HERE AND WOULD NOT BE ON THE APP LEG, because
/// the two legs looking different is the first thing that will read as drift:
/// this matcher has a second, per-binding discriminator the app leg does not —
/// the SITE. A candidate must already carry this binding's own keyword or host
/// in its title, so the worst `Any` can do is toggle a window that is showing
/// this key's own website. `try_focus_or_minimize` has no such filter: every
/// window of the executable is a candidate there, which is why the owner's
/// unpinned rule (decline what another binding has claimed) lives on that leg
/// and only that leg.
#[cfg(windows)]
fn url_focus_or_minimize(url: &str, binding: &KeyBinding) -> bool {
    use crate::browser_profiles::BrowserRoute;

    let Some((keyword, host)) = url_match_keys(url) else {
        log::info!("url_focus: {url:?} has no safe title keyword — launching normally");
        return false;
    };

    let default_stem = || match browser_stem() {
        Some(s) => Some(s),
        None => {
            log::info!("url_focus: no known browser resolved — launching normally");
            None
        }
    };

    // ONE route decision, read by both halves below — the browser to search and
    // the profile to demand must come from the same answer, not from two calls
    // that could be edited apart.
    let route = crate::browser_profiles::route_for(binding, &real_fs());
    let rule = url_rule_for_route(&route, binding, &real_fs());
    let stem = match &route {
        BrowserRoute::Specific { exe } => launch_stem(exe),
        // Both fallback routes launch through `run_browser`, so both search the
        // OS DEFAULT browser's windows — exactly as this function did before
        // any of the profile work existed.
        BrowserRoute::Default | BrowserRoute::PinnedBrowserMissing { .. } => {
            let Some(s) = default_stem() else { return false };
            s
        }
    };

    // Only the profile-aware rules read window properties, so only they pay for
    // COM. Declared here so it outlives the enumeration below.
    let _com = unsafe { ComGuard::for_rule(&rule) };

    use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, IsWindowVisible};

    struct UrlSearch {
        stem: String,
        keyword: String,
        host: String,
        /// Which of that browser's windows this binding is allowed to touch.
        rule: ProfileRule,
        found: Option<HWND>,
        seen: Vec<String>,
    }

    unsafe extern "system" fn url_enum_cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
        use windows::Win32::System::Threading::{
            OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
            PROCESS_QUERY_LIMITED_INFORMATION,
        };
        use windows::Win32::UI::WindowsAndMessaging::{
            GetWindowTextW, GetWindowThreadProcessId,
        };

        if !IsWindowVisible(hwnd).as_bool() {
            return BOOL(1);
        }
        let p = &mut *(lparam.0 as *mut UrlSearch);

        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 || pid == std::process::id() {
            return BOOL(1);
        }

        let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return BOOL(1);
        };
        let mut buf = [0u16; 260];
        let mut size = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_FORMAT(0),
            windows::core::PWSTR(buf.as_mut_ptr()),
            &mut size,
        );
        let _ = windows::Win32::Foundation::CloseHandle(handle);
        if ok.is_err() {
            return BOOL(1);
        }
        let path = String::from_utf16_lossy(&buf[..size as usize]);
        let this_stem = std::path::Path::new(&path)
            .file_stem()
            .map(|f| f.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if this_stem != p.stem {
            return BOOL(1);
        }

        // Browser window title is "<active tab title> - Brave". A captionless
        // window is a helper, not a browsing window.
        let mut tbuf = [0u16; 512];
        let n = GetWindowTextW(hwnd, &mut tbuf);
        if n <= 0 {
            return BOOL(1);
        }
        let title = String::from_utf16_lossy(&tbuf[..n as usize]).to_lowercase();

        if title.contains(&p.keyword) || title.contains(&p.host) {
            // The title matched — but is this window even this binding's?
            // Same fail-safe as the app path: a window that cannot PROVE it
            // belongs here is left alone, and the enumeration CONTINUES
            // (BOOL(1), not BOOL(0)) so the right profile's window sitting
            // behind it can still be found.
            if !matches!(p.rule, ProfileRule::Any)
                && !evidence_satisfies(&window_profile_evidence(hwnd), &p.rule)
            {
                p.seen.push(format!("{title} [a different profile]"));
                return BOOL(1);
            }
            p.found = Some(hwnd);
            return BOOL(0); // stop
        }
        p.seen.push(title);
        BOOL(1)
    }

    let mut payload = UrlSearch {
        stem: stem.clone(),
        keyword: keyword.clone(),
        host: host.clone(),
        rule: rule.clone(),
        found: None,
        seen: Vec::new(),
    };
    unsafe {
        let _ = EnumWindows(
            Some(url_enum_cb),
            LPARAM(&mut payload as *mut UrlSearch as isize),
        );
    }

    if let Some(hwnd) = payload.found {
        unsafe {
            let is_active = GetForegroundWindow() == hwnd;
            let is_minimized = IsIconic(hwnd).as_bool();
            if is_active && !is_minimized {
                log::info!(
                    "url_focus: {stem} window showing {keyword:?} is foreground — minimizing {hwnd:?}"
                );
                let _ = ShowWindow(hwnd, SW_MINIMIZE);
            } else {
                log::info!(
                    "url_focus: {stem} window showing {keyword:?} — restoring {hwnd:?}"
                );
                let _ = ShowWindow(hwnd, SW_RESTORE);
                force_foreground(hwnd);
            }
        }
        true
    } else {
        // info!, not debug! — a debug-level diagnostic does not exist in the
        // shipped log, which is exactly when it is needed (PROBLEM 38).
        log::info!(
            "url_focus: no {stem} window titled {keyword:?} — launching {url:?}. Titles seen: {:?}",
            payload.seen
        );
        false
    }
}

#[cfg(not(windows))]
fn url_focus_or_minimize(_url: &str, _binding: &KeyBinding) -> bool {
    false
}

/// Why a cached HWND may not be acted on. One variant per fact
/// `enum_callback` asserts, so a reader can see at a glance that the two paths
/// test the same things.
#[derive(Debug, Clone, PartialEq, Eq)]
enum CacheEvict {
    /// `IsWindow` says the handle is gone.
    Dead,
    /// The handle is alive but we could not name its owning process — pid 0, a
    /// process we may not open, a failed image-name query. `enum_callback`
    /// never selects such a window (it `continue`s), so neither may we.
    UnprovableIdentity,
    /// THE RECYCLED HANDLE. Alive, nameable — and running a different program
    /// than the binding names.
    WrongExecutable,
    /// explorer.exe, and not class `CabinetWClass`: shell infrastructure.
    ShellWindow,
    /// A non-explorer window with no caption — helper/IME/tray, not the app.
    NoTitle,
    /// The window can no longer prove it belongs to this binding's profile.
    WrongProfile,
}

/// May we act on a CACHED handle, or must it be evicted and re-enumerated?
///
/// PROBLEM 226 — THE HOLE THIS CLOSES. The cache-hit branch used to re-validate
/// exactly two things: the explorer/`CabinetWClass` class rule (gated on the
/// BINDING's stem, `exe_lower == "explorer"`) and the profile (gated on
/// `!matches!(rule, ProfileRule::Any)`). For the overwhelmingly common case — a
/// non-browser binding on `ProfileRule::Any`, which the `Any` doc above calls
/// "every binding that existed before browser profiles, every non-browser
/// binding" — BOTH guards short-circuit and the only surviving check was bare
/// `IsWindow`.
///
/// So: bind Space+N to `notepad`, press it (HWND `0x000A1234` is cached), close
/// Notepad, and let Windows recycle that handle value onto some other process's
/// top-level window — the session handle table is shared, which is the entire
/// reason NATIVE_SAFETY.md rule 3 exists. The next Space+N found `alive == true`,
/// skipped the class filter (the BINDING is not explorer, so it never looked at
/// what the handle points at NOW), skipped the profile filter (`Any`), and went
/// straight to `SW_MINIMIZE` / `SW_RESTORE` + `force_foreground`. If the recycled
/// handle belongs to explorer.exe shell infrastructure (`Shell_TrayWnd`,
/// `WorkerW`, `XamlExplorerHostIslandWindow`), that is the 2026-08-10 touchpad
/// incident reproduced through a path that no longer looks at classes at all.
/// The comment that used to sit on the profile check claimed a recycled handle
/// "fails SAFE" here; it was true only for `Pinned`/`Unpinned`.
///
/// THE RULE, and it is the only one that keeps the two paths honest: **the
/// cache-hit branch may act only on a window the fresh enumeration would also
/// have selected.** Every arm below is the cache-side mirror of a `return
/// BOOL(1)` in `enum_callback`, in the same order — safety filters first,
/// acceptance last.
///
/// Pure, so the recycled-HWND case can be tested without recycling a handle.
///
/// * `live_stem` — the lowercased exe stem of the process that owns the handle
///   **right now** (`window_process_identity`), `None` when unprovable.
/// * `class` / `has_title` — read off the live window, not remembered.
/// * `profile_proven` — LAZY, and deliberately so: it is the only expensive
///   fact (a Chromium property-store read behind COM), and the enum path pays
///   for it last too. A window that already failed a safety filter must not
///   cost a property read, and `ProfileRule::Any` must not cost one at all.
fn cached_window_verdict(
    alive: bool,
    live_stem: Option<&str>,
    class: &str,
    has_title: bool,
    binding_stem: &str,
    profile_proven: impl FnOnce() -> bool,
) -> Result<(), CacheEvict> {
    if !alive {
        return Err(CacheEvict::Dead);
    }
    let Some(live_stem) = live_stem else {
        return Err(CacheEvict::UnprovableIdentity);
    };
    if live_stem != binding_stem {
        return Err(CacheEvict::WrongExecutable);
    }
    // NATIVE_SAFETY rule 1/2 — a POSITIVE filter, and keyed on what the handle
    // points at NOW. Keying it on the binding's stem (which is what the old
    // code did) is not a class check at all: it asks "did the user bind
    // explorer?", not "is this the taskbar?".
    if live_stem == "explorer" {
        if class != "CabinetWClass" {
            return Err(CacheEvict::ShellWindow);
        }
    } else if !has_title {
        return Err(CacheEvict::NoTitle);
    }
    if !profile_proven() {
        return Err(CacheEvict::WrongProfile);
    }
    Ok(())
}

/// Returns `true` if an existing window was found and acted upon.
///
/// `rule` is the binding's identity beyond its executable — see `ProfileRule`.
/// On `ProfileRule::Any`, which is every binding that has nothing to do with a
/// pinned browser profile, this reads no Chromium window PROPERTY and
/// initialises no COM apartment — the profile machinery is still entirely off
/// the path. What it does now do on every path, `Any` included, is re-prove the
/// cached window's IDENTITY before touching it (`cached_window_verdict`): the
/// owning process's exe stem, the explorer class rule and the caption rule, the
/// same three facts a fresh enumeration asserts. That is four cheap syscalls on
/// a keypress path, and it is the difference between "we re-validated the
/// handle" being true and being a comment.
#[cfg(windows)]
fn try_focus_or_minimize(exe_name: &str, rule: &ProfileRule) -> bool {
    // Match by file STEM (no extension): bindings may store a .lnk path (the
    // app picker keeps shortcuts whose arguments matter), and "discord.lnk"
    // must still match the running "discord.exe" process.
    let exe_lower = std::path::Path::new(exe_name)
        .file_stem()
        .map(|f| f.to_string_lossy().to_lowercase())
        .unwrap_or_else(|| exe_name.to_lowercase());

    // THE KEY — the BINDING's identity, not the executable's. Built by one
    // named function and used at both the read below and the write further
    // down, because a read site and a write site that derive the same key
    // separately are a bug waiting for someone to edit one of them.
    let key = cache_key(&exe_lower, rule);

    // Declared FIRST so it outlives every early return under it.
    let _com = unsafe { ComGuard::for_rule(rule) };

    // Check Cache first
    let cache_lock = get_app_cache();
    let mut cache = cache_lock.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(&hwnd_raw) = cache.get(&key) {
        let hwnd = windows::Win32::Foundation::HWND(hwnd_raw as *mut _);
        unsafe {
            let alive = IsWindow(hwnd).as_bool();

            // NATIVE_SAFETY.md rule 3 — "never trust a cached HWND". Re-read
            // every fact from the LIVE window; remember nothing. See
            // `cached_window_verdict` for why the two facts this branch used to
            // check were not enough, and for the recycled-handle failure that
            // reached `SW_MINIMIZE` through it.
            //
            // THIS IS THE BRANCH THE REPORTED BUG ACTUALLY FIRED ON. The
            // owner's log at 23:44:10.136 shows "Action: Minimize" with no
            // "(Enum)" suffix: a cache hit, no enumeration, no chance for any
            // matcher to have an opinion.
            //
            // COST, because this is the Space-hold dispatch path: on a cache
            // hit, one `GetWindowThreadProcessId` + `OpenProcess` +
            // `QueryFullProcessImageNameW` + `CloseHandle` (~0.05 ms measured
            // for the same quartet in `enum_callback`) and one
            // `GetClassNameW`/`GetWindowTextLengthW`. That is a rounding error
            // beside the `ShowWindow` it guards, and it is the ONLY thing
            // standing between a recycled handle and the taskbar.
            let live = if alive { window_process_identity(hwnd) } else { None };
            let (class, has_title) = if alive {
                let mut cls_buf = [0u16; 64];
                let n = windows::Win32::UI::WindowsAndMessaging::GetClassNameW(hwnd, &mut cls_buf);
                let cls = String::from_utf16_lossy(&cls_buf[..n.max(0) as usize]);
                let titled = windows::Win32::UI::WindowsAndMessaging::GetWindowTextLengthW(hwnd) != 0;
                (cls, titled)
            } else {
                (String::new(), false)
            };
            match cached_window_verdict(
                alive,
                live.as_ref().map(|(_, stem)| stem.as_str()),
                &class,
                has_title,
                &exe_lower,
                // `Any` proves nothing and reads nothing — no property store,
                // no COM. Identical to the enum path's short-circuit, and
                // reached only after every cheaper filter has passed.
                || {
                    matches!(rule, ProfileRule::Any)
                        || evidence_satisfies(&window_profile_evidence(hwnd), rule)
                },
            ) {
                Err(why) => {
                    // Dead handles are routine (the app was closed) and say
                    // nothing; every other eviction means the cache was about
                    // to act on the wrong window, which is worth a line.
                    if why != CacheEvict::Dead {
                        let (pid, stem) = live
                            .map(|(p, s)| (p, s))
                            .unwrap_or((0, "<unprovable>".into()));
                        log::info!(
                            "cascade: cached {hwnd:?} for {key:?} did NOT re-validate \
                             ({why:?}) — it is now pid {pid} '{stem}' class {class:?} \
                             (titled: {has_title}), not this binding's window. Dropping it \
                             and enumerating again rather than acting on it."
                        );
                    }
                    cache.remove(&key);
                }
                Ok(()) => {
                    let is_active = GetForegroundWindow() == hwnd;
                    let is_minimized = IsIconic(hwnd).as_bool();

                    if is_active && !is_minimized {
                        log::info!("Event: Space+? | Target: {} | HWND: {:?} | Action: Minimize | Rule: {:?}", exe_name, hwnd, rule);
                        let _ = ShowWindow(hwnd, SW_MINIMIZE);
                    } else {
                        log::info!("Event: Space+? | Target: {} | HWND: {:?} | Action: Restore | Rule: {:?}", exe_name, hwnd, rule);
                        let _ = ShowWindow(hwnd, SW_RESTORE);
                        force_foreground(hwnd);
                    }
                    return true;
                }
            }
        }
    }
    drop(cache); // Release lock before EnumWindows

    let mut search = StemSearch {
        stem: exe_lower.clone(),
        rule: rule.clone(),
        found: None,
        declined: Vec::new(),
    };

    unsafe {
        // Enumerate all top-level windows looking for our exe.
        //
        // A pointer to this stack value rather than a Box, matching the three
        // other enumerations in this file. EnumWindows is synchronous, so the
        // callback cannot outlive this scope and there is nothing to reclaim
        // afterwards — which also removes the leak the boxed version once had
        // (`Box::into_raw` with no matching `from_raw`, a String and an Arc per
        // uncached press, all day, in a tray app).
        let _ = EnumWindows(
            Some(enum_callback),
            windows::Win32::Foundation::LPARAM(&mut search as *mut StemSearch as isize),
        );
    }

    if let Some(hwnd) = search.found {
        // Store in cache for next time
        get_app_cache()
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(key.clone(), hwnd.0 as isize);

        unsafe {
            let is_active = GetForegroundWindow() == hwnd;
            let is_minimized = IsIconic(hwnd).as_bool();

            if is_active && !is_minimized {
                log::info!("Event: Space+? | Target: {} | HWND: {:?} | Action: Minimize (Enum) | Rule: {:?}", exe_name, hwnd, rule);
                let _ = ShowWindow(hwnd, SW_MINIMIZE);
            } else {
                log::info!("Event: Space+? | Target: {} | HWND: {:?} | Action: Restore (Enum) | Rule: {:?}", exe_name, hwnd, rule);
                let _ = ShowWindow(hwnd, SW_RESTORE);
                force_foreground(hwnd);
            }
        }
        return true;
    }

    // The `seen`-list idiom from `aumid_focus_or_minimize`, and for the same
    // reason: without it, "this key launched instead of toggling" is
    // indistinguishable from "there were no windows of that browser at all",
    // and the only way to tell them apart is to add logging and ask the owner
    // to reproduce — a whole round trip. info!, not debug!, because debug lines
    // are filtered out of the shipped log, which is exactly when this line is
    // needed (PROBLEM 38).
    if !search.declined.is_empty() {
        log::info!(
            "cascade: {} {exe_lower} window(s) matched the executable but could not prove \
             they belong to this binding ({rule:?}) — launching rather than touching them: \
             {:?}",
            search.declined.len(),
            search.declined
        );
    }
    false
}

#[cfg(not(windows))]
fn try_focus_or_minimize(_exe_name: &str, _rule: &ProfileRule) -> bool { false }

/// The payload every stem search hands to `enum_callback`.
///
/// Shared by `try_focus_or_minimize` (which toggles) and
/// `find_window_by_exe_stem` (which only ever finds), so the two cannot
/// disagree about which window belongs to a binding. They did disagree, in a
/// way that was hard to see: a CORRECT launch into Profile 2 could be followed
/// by the post-launch watcher raising Profile 1's window, because the raise
/// went through the same profile-blind callback.
#[cfg(windows)]
struct StemSearch {
    /// The lowercased exe stem the binding names.
    stem: String,
    /// Which of that executable's windows this binding may touch.
    rule: ProfileRule,
    found: Option<HWND>,
    /// Windows that matched the executable and passed every safety filter but
    /// could not prove they belong to THIS binding. Diagnostics only — see the
    /// log line at the end of `try_focus_or_minimize`.
    declined: Vec<String>,
}

#[cfg(windows)]
unsafe extern "system" fn enum_callback(
    hwnd: HWND,
    lparam: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::BOOL {
    use windows::Win32::Foundation::BOOL;
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;

    if !IsWindowVisible(hwnd).as_bool() {
        return BOOL(1); // continue
    }

    let style = GetWindowLongW(hwnd, GWL_STYLE) as u32;
    if (style & WS_VISIBLE.0) == 0 {
        return BOOL(1);
    }

    let payload = &mut *(lparam.0 as *mut StemSearch);

    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if pid == 0 {
        return BOOL(1);
    }

    let my_pid = std::process::id();
    if pid == my_pid {
        return BOOL(1);
    }

    let handle = match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
        Ok(h) => h,
        Err(_) => return BOOL(1),
    };

    let mut buf = [0u16; 260];
    let mut size = buf.len() as u32;
    let pwstr = windows::core::PWSTR(buf.as_mut_ptr());
    let ok = QueryFullProcessImageNameW(handle, PROCESS_NAME_FORMAT(0), pwstr, &mut size);
    let _ = windows::Win32::Foundation::CloseHandle(handle);

    if ok.is_err() {
        return BOOL(1);
    }

    let path = String::from_utf16_lossy(&buf[..size as usize]);
    let exe = std::path::Path::new(&path)
        .file_name()
        .map(|f| f.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    // Target is a STEM (see try_focus_or_minimize) — compare stems.
    let stem = std::path::Path::new(&path)
        .file_stem()
        .map(|f| f.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    if stem == payload.stem.as_str() {
        // ==================================================================
        // NATIVE-SAFETY RULE (see NATIVE_SAFETY.md — incident 2026-08-10):
        // explorer.exe IS the Windows shell. Its visible top-level windows
        // include the taskbar, the desktop, the Task-View/gesture overlays
        // and assorted helpers (ThumbnailDeviceHelperWnd, ...). Minimizing or
        // force-foregrounding those broke the user's 3/4-finger touchpad
        // gestures and window management until Explorer was restarted.
        //
        // A class DENYLIST proved unreliable (each test found a new helper
        // class), so for explorer.exe we use a POSITIVE filter: the only
        // windows we may ever touch are real File Explorer file-manager
        // windows, class "CabinetWClass".
        //
        // THIS BLOCK RUNS BEFORE THE PROFILE CHECK BELOW, and that order is
        // deliberate: safety filters first, acceptance last, always.
        // ==================================================================
        let mut cls_buf = [0u16; 64];
        let n = windows::Win32::UI::WindowsAndMessaging::GetClassNameW(hwnd, &mut cls_buf);
        let cls = String::from_utf16_lossy(&cls_buf[..n.max(0) as usize]);

        if exe == "explorer.exe" {
            if cls != "CabinetWClass" {
                return BOOL(1); // shell infrastructure — never touch, keep looking
            }
        } else {
            // For every other app: skip captionless tool/helper windows.
            // Real application main windows have a title; invisible helpers
            // (IME hosts, trays, splash leftovers) usually don't.
            use windows::Win32::UI::WindowsAndMessaging::GetWindowTextLengthW;
            if GetWindowTextLengthW(hwnd) == 0 {
                return BOOL(1);
            }
        }

        // ==================================================================
        // PROFILE EVIDENCE — the fix for 2026-08-26's wrong minimise.
        //
        // Everything above proves the window belongs to the right EXECUTABLE
        // and is safe to touch at all. That was the entire test until now, and
        // two Brave bindings are the same executable.
        //
        // Note the BOOL(1): a window that is not this binding's CONTINUES the
        // enumeration instead of ending it. Stopping here would mean the right
        // window sitting behind the wrong one is never reached, which is
        // exactly what the owner's rule for unpinned bindings requires — B
        // takes whichever Brave window is not N's.
        //
        // `ProfileRule::Any` short-circuits before any property is read, so
        // every binding that predates this feature pays nothing.
        // ==================================================================
        if !matches!(payload.rule, ProfileRule::Any) {
            let evidence = window_profile_evidence(hwnd);
            if !evidence_satisfies(&evidence, &payload.rule) {
                payload.declined.push(format!("{hwnd:?} {evidence:?}"));
                return BOOL(1);
            }
        }

        payload.found = Some(hwnd);
        return BOOL(0); // stop enumeration
    }

    BOOL(1)
}

/// Attempt to launch an app by exe name or absolute path.
///
/// Priority:
///   1. If `exe_name` is already an absolute path → use directly (dashboard-set paths)
///   2. Protocol URI shortcut (discord://, spotify://, etc.)
///   3. Known-app lookup table via resolve_path()
///   4. Registry HKLM/HKCU App Paths fallback (inside resolve_path)
/// `shell:AppsFolder\<AUMID>` (Microsoft Store app) or any other shell: verb.
fn is_shell_target(target: &str) -> bool {
    target.len() > 6 && target[..6].eq_ignore_ascii_case("shell:")
}

// ===========================================================================
// THE LAUNCH SPINE (2026-08-29, PROBLEM 216)
// ===========================================================================
//
// THE INVARIANT, and the whole reason this section exists:
//
//   EVERY launch branch must first attempt focus/minimize using an identity
//   derived from the SAME resolution the launch will perform. A branch that
//   can launch but cannot match is a bug by construction.
//
// It was violated for months and nobody could see it, because the violation is
// INVISIBLE FROM THE MATCHER. `whatsapp://` launched through `launch_app_inner`
// case 2 while the match leg had already asked the stem matcher for a process
// called `whatsapp` — and the Store build of WhatsApp runs as `WhatsApp.Root`.
// Every previous fix improved a matcher this shape never reaches, so every
// press re-launched. The identical shape exists for a Start-Menu `.lnk` whose
// target exe has a different name from the shortcut.
//
// THE STRUCTURAL GUARANTEE. Resolution now returns ONE value carrying all
// three halves — the shape the launch dispatches on, how to FIND the window,
// and what the post-launch watcher should look for:
//
//   * `TargetShape` is the ONLY thing `launch_app_inner` may dispatch on, and
//     `LaunchPlan` is the only thing that produces one.
//   * `LaunchPlan` has exactly TWO constructors, `matchable` and `unmatchable`.
//     `matchable` takes its first identity BY VALUE, so an empty identity list
//     cannot be spelled; `unmatchable` demands a reason string that is logged
//     at `info!`.
//
// So adding a fifth branch next year means adding a `TargetShape` variant,
// which makes `plan_for` and `launch_app_inner` both fail to compile until the
// author has said how the new shape is matched and how it is raised. "I forgot
// to write the matching code" is no longer expressible — the compiler asks, and
// the only way to answer "it cannot be matched" is to name the reason out loud.

/// Which of the launcher's resolution ladders a binding string takes.
///
/// This IS the launch action selector: `launch_app_inner` matches on it and on
/// nothing else. Kept in the same order as the ladder it replaced, so the
/// classification and the launch can never disagree about which case applies —
/// they are now the same `match`.
#[derive(Debug, Clone, PartialEq, Eq)]
enum TargetShape {
    /// `shell:AppsFolder\<AUMID>`, or any other `shell:` verb.
    ShellVerb,
    /// An absolute path — `.exe`, `.lnk`, or a document.
    AbsolutePath,
    /// A bare name this app knows a protocol URI for (`whatsapp.exe` →
    /// `whatsapp://`). Carries the URI so the launch does not re-derive it.
    ProtocolUri(String),
    /// A bare exe name, resolved through the known-paths table, App Paths,
    /// PATH and finally the Start Menu.
    BareName,
}

/// Classify a binding string. PURE — no registry, no filesystem, no COM — so
/// it is cheap enough to run on the Space-hold path and testable without a
/// machine.
fn classify_target(target: &str) -> TargetShape {
    if is_shell_target(target) {
        return TargetShape::ShellVerb;
    }
    if std::path::Path::new(target).is_absolute() {
        return TargetShape::AbsolutePath;
    }
    if let Some(uri) = protocol_uri(target) {
        return TargetShape::ProtocolUri(uri);
    }
    TargetShape::BareName
}

/// How to FIND this binding's window. Every entry is fed to a matcher that
/// ALREADY EXISTS — deliberately, because those two carry the NATIVE_SAFETY
/// protections (explorer/`CabinetWClass` positive filter, captionless-window
/// skip, own-PID skip, cloaked-window skip, HWND-recycle re-validation) and
/// PROBLEM 207's profile discipline. A third matcher would have to re-earn all
/// of that, and would drift.
#[derive(Debug, Clone, PartialEq, Eq)]
enum MatchIdentity {
    /// Match by process exe stem → `try_focus_or_minimize`, which stems
    /// whatever it is given.
    ///
    /// Holds the BINDING STRING for the identity derived from the binding
    /// itself (`C:\…\Discord.exe`, `discord.exe`) and a bare stem for the ones
    /// derived from a registry handler or a resolution. That is deliberate:
    /// `try_focus_or_minimize` prints its argument as the `Target:` field of
    /// the `Event: Space+? | Target: … | HWND: … | Action: … | Rule: …` line,
    /// which is the line the owner reads to see the cascade working. Handing it
    /// a pre-stemmed string would quietly delete the path from that line.
    ExeStem(String),
    /// Match by AUMID, package family, or Apps-folder target →
    /// `aumid_focus_or_minimize`. Held in `shell:AppsFolder\…` form so that
    /// matcher's fallback 3 can still parse it through the shell namespace.
    Aumid(String),
    /// Resolve this binding the way the LAUNCH will (known-paths table → App
    /// Paths → PATH → Start-Menu `.lnk` → the shortcut's target exe) and match
    /// on the stem that produces.
    ///
    /// Lazy on purpose. Resolution can walk two Start-Menu trees, and this runs
    /// on the Space-hold path; it is only ever reached after the cheap stem has
    /// already missed, which is exactly the press that was about to resolve
    /// anyway in order to launch. Memoised after that (`resolved_target_stem`).
    ResolvedExeStem(String),
}

/// The identities to try, in order, cheapest first — or a named reason why
/// there are none.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Identities {
    /// Non-empty by construction: see `LaunchPlan::matchable`.
    Try(Vec<MatchIdentity>),
    /// An explicit, named, LOGGED decision. Not something a branch can fall
    /// into by omitting code.
    Unmatchable(String),
}

/// What the post-launch watcher (PROBLEM 170) should look for.
#[derive(Debug, Clone, PartialEq, Eq)]
enum RaiseIdentity {
    /// Poll for a window of these exe stems, in order.
    ExeStems(Vec<String>),
    /// Poll for whatever the launch resolution produces, falling back to the
    /// binding's own stem when it resolves to nothing.
    Resolved(String),
    /// Do not watch, and say why.
    Skip(&'static str),
}

/// ONE resolution, all three halves. See the section header for why this is a
/// struct and not three loose functions.
#[derive(Debug, Clone, PartialEq, Eq)]
struct LaunchPlan {
    shape: TargetShape,
    identity: Identities,
    raise: RaiseIdentity,
}

impl LaunchPlan {
    /// `first` is by value: a zero-identity plan cannot be spelled through this
    /// constructor, which is the point.
    fn matchable(
        shape: TargetShape,
        first: MatchIdentity,
        rest: Vec<MatchIdentity>,
        raise: RaiseIdentity,
    ) -> Self {
        let mut ids = Vec::with_capacity(1 + rest.len());
        ids.push(first);
        for r in rest {
            if !ids.contains(&r) {
                ids.push(r);
            }
        }
        LaunchPlan { shape, identity: Identities::Try(ids), raise }
    }

    fn unmatchable(shape: TargetShape, reason: impl Into<String>) -> Self {
        LaunchPlan {
            shape,
            identity: Identities::Unmatchable(reason.into()),
            raise: RaiseIdentity::Skip("nothing about this binding names a window"),
        }
    }
}

/// The stem of a binding string, or `None` when it names no file at all.
fn stem_of(target: &str) -> Option<String> {
    let s = launch_stem(target);
    if s.is_empty() { None } else { Some(s) }
}

/// THE ONLY function that builds a `LaunchPlan`.
///
/// Each arm keeps its own RESOLUTION — that is the part that legitimately
/// differs per shape. What no arm keeps any more is its own CONTROL FLOW.
fn resolve_launch_plan(target: &str) -> LaunchPlan {
    let shape = classify_target(target);
    match &shape {
        // Unchanged from before this section existed: a Store target has always
        // gone to the AUMID matcher, and its raise has always been skipped
        // because a packaged window belongs to a host process, not to anything
        // named after the app.
        TargetShape::ShellVerb => LaunchPlan::matchable(
            shape.clone(),
            MatchIdentity::Aumid(target.to_string()),
            Vec::new(),
            RaiseIdentity::Skip(
                "a Store/packaged activation owns no process named after the app — the \
                 shell foregrounds it and AllowSetForegroundWindow covers the handoff",
            ),
        ),

        TargetShape::AbsolutePath => match stem_of(target) {
            None => LaunchPlan::unmatchable(shape.clone(), "the path names no file"),
            Some(stem) => {
                // A `.lnk` is the shape the app picker stores whenever a
                // shortcut's arguments matter, and a shortcut's NAME is not its
                // target's process name ("NVIDIA GeForce Experience.lnk" starts
                // "NVIDIA Share.exe"). The second identity is the shortcut's
                // actual target — the same file the launch will run.
                let is_lnk = std::path::Path::new(target)
                    .extension()
                    .map(|x| x.eq_ignore_ascii_case("lnk"))
                    .unwrap_or(false);
                let rest = if is_lnk {
                    vec![MatchIdentity::ResolvedExeStem(target.to_string())]
                } else {
                    Vec::new()
                };
                let raise = if is_lnk {
                    RaiseIdentity::Resolved(target.to_string())
                } else {
                    RaiseIdentity::ExeStems(vec![stem])
                };
                LaunchPlan::matchable(
                    shape.clone(),
                    MatchIdentity::ExeStem(target.to_string()),
                    rest,
                    raise,
                )
            }
        },

        // THE BRANCH THE BUG LIVED IN. It had no match leg at all: the cascade
        // asked the stem matcher for the BINDING's stem, the launcher activated
        // a protocol, and for a Store-packaged handler those two are different
        // programs. `scheme_identities` asks the OS who actually handles the
        // scheme and hands the answer to whichever existing matcher can consume
        // it — AUMID for a packaged handler, exe stem for a classic one.
        TargetShape::ProtocolUri(uri) => {
            let scheme = scheme_of_uri(uri).unwrap_or_default().to_string();
            let resolved = scheme_identities(&scheme);
            // The binding's own stem stays FIRST. It is what this branch has
            // always tried, it is free, and for a classic handler bound by bare
            // name (discord.exe → discord://) it is already the right answer —
            // so the working path stays byte-identical.
            let cheap = stem_of(target).map(|_| MatchIdentity::ExeStem(target.to_string()));
            let raise = match resolved.iter().find_map(|i| match i {
                MatchIdentity::ExeStem(s) => Some(s.clone()),
                _ => None,
            }) {
                // A classic handler: raise the exe the REGISTRY named, not the
                // exe the binding happens to be called.
                Some(handler_stem) => {
                    let mut stems = vec![handler_stem];
                    if let Some(s) = stem_of(target) {
                        if !stems.contains(&s) {
                            stems.push(s);
                        }
                    }
                    RaiseIdentity::ExeStems(stems)
                }
                None if resolved.iter().any(|i| matches!(i, MatchIdentity::Aumid(_))) => {
                    RaiseIdentity::Skip(
                        "this scheme is handled by a PACKAGED app, which owns no process \
                         named after the binding — the shell foregrounds it",
                    )
                }
                // Nothing resolved: today's behaviour exactly.
                None => match stem_of(target) {
                    Some(s) => RaiseIdentity::ExeStems(vec![s]),
                    None => RaiseIdentity::Skip("the binding names no file"),
                },
            };
            match cheap {
                Some(first) => LaunchPlan::matchable(shape.clone(), first, resolved, raise),
                None => match resolved.split_first() {
                    Some((first, rest)) => LaunchPlan::matchable(
                        shape.clone(),
                        first.clone(),
                        rest.to_vec(),
                        raise,
                    ),
                    None => LaunchPlan::unmatchable(
                        shape.clone(),
                        format!(
                            "the binding names no file and nothing on this machine is \
                             registered to handle {scheme}://"
                        ),
                    ),
                },
            }
        }

        TargetShape::BareName => match stem_of(target) {
            None => LaunchPlan::unmatchable(shape.clone(), "the binding names no file"),
            Some(_) => LaunchPlan::matchable(
                shape.clone(),
                MatchIdentity::ExeStem(target.to_string()),
                vec![MatchIdentity::ResolvedExeStem(target.to_string())],
                RaiseIdentity::Resolved(target.to_string()),
            ),
        },
    }
}

/// THE ONE MATCH LEG for every app binding.
///
/// `smart_cascade`'s primary arm and its Founders arm both call this, and there
/// is no other way in — the two `if is_shell_target(...) { aumid... } else {
/// stem... }` ladders they used to carry are gone. That shape is what let a
/// third case (protocol URIs) exist with no match leg at all.
fn app_focus_or_minimize(target: &str, rule: &ProfileRule) -> bool {
    let plan = resolve_launch_plan(target);
    let ids = match &plan.identity {
        Identities::Unmatchable(why) => {
            // info!, never debug! — debug is filtered out of the shipped log,
            // which is exactly when this line is needed (PROBLEM 38). Silent
            // unmatchability is what made the original bug invisible.
            log::info!(
                "cascade: {target:?} ({:?}) has no window identity — {why}. Launching \
                 unconditionally, which is what this binding did before.",
                plan.shape
            );
            return false;
        }
        Identities::Try(ids) => ids,
    };

    // Deduped by STEM, not by the string, because identity #0 carries the whole
    // binding string (see `MatchIdentity::ExeStem`) while a resolved identity
    // carries a bare stem — and `C:\…\Discord.exe` and `discord` are the same
    // enumeration. A duplicate here is a duplicate `EnumWindows` on the
    // Space-hold path.
    let mut tried_stems: Vec<String> = Vec::new();
    for id in ids {
        match id {
            MatchIdentity::ExeStem(target) => {
                let stem = launch_stem(target);
                if tried_stems.contains(&stem) {
                    continue;
                }
                tried_stems.push(stem);
                if try_focus_or_minimize(target, rule) {
                    return true;
                }
            }
            MatchIdentity::Aumid(aumid) => {
                if aumid_focus_or_minimize(aumid, rule) {
                    return true;
                }
            }
            MatchIdentity::ResolvedExeStem(name) => {
                let Some(stem) = resolved_target_stem(name) else {
                    log::info!(
                        "cascade: {name:?} could not be resolved to a real program, so there \
                         is no second identity to match on — launching."
                    );
                    continue;
                };
                if tried_stems.contains(&stem) {
                    continue;
                }
                log::info!(
                    "cascade: {name:?} resolves to a program called {stem:?} — matching on \
                     that as well, because that is the process the launch will create."
                );
                tried_stems.push(stem.clone());
                if try_focus_or_minimize(&stem, rule) {
                    return true;
                }
            }
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Resolution helpers — the per-shape half that legitimately differs
// ---------------------------------------------------------------------------

/// `whatsapp://` → `whatsapp`. Pure.
///
/// The length guard is not pedantry: `C:\Users\…` splits at a colon too, and a
/// one-letter "scheme" is a Windows DRIVE. RFC 3986 schemes are at least two
/// characters and alphanumeric with `+-.`, which excludes drive letters exactly.
fn scheme_of_uri(uri: &str) -> Option<&str> {
    let s = uri
        .split_once("://")
        .map(|(s, _)| s)
        .or_else(|| uri.split_once(':').map(|(s, _)| s))?;
    let ok = s.len() >= 2
        && s.starts_with(|c: char| c.is_ascii_alphabetic())
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.');
    if ok { Some(s) } else { None }
}

/// The executable out of a `shell\open\command` value. Pure, and the two
/// shapes are BOTH real on this machine (measured 2026-08-29):
///
/// ```text
/// discord   "C:\Users\…\Discord.exe" --url -- "%1"      <- quoted
/// spotify   "C:\Users\…\spotify.exe" --protocol-uri="%1" <- quoted
/// ```
///
/// The unquoted shape (`C:\Windows\notepad.exe %1`) is the classic one and is
/// still everywhere in HKCR, so both are parsed. A parser that only understood
/// the quoted form would return nothing for those, silently, forever — the
/// same failure mode PROBLEM 207 measured when Chrome quoted its profile folder
/// and Edge did not.
fn exe_from_shell_open_command(cmd: &str) -> Option<String> {
    let cmd = cmd.trim();
    if cmd.is_empty() {
        return None;
    }
    if let Some(rest) = cmd.strip_prefix('"') {
        let (inside, _) = rest.split_once('"')?;
        return if inside.is_empty() { None } else { Some(inside.to_string()) };
    }
    // Unquoted. Prefer cutting after a `.exe`, because an unquoted path MAY
    // still contain spaces; only fall back to the first whitespace.
    let lower = cmd.to_lowercase();
    if let Some(pos) = lower.find(".exe") {
        return Some(cmd[..pos + 4].to_string());
    }
    let first = cmd.split_whitespace().next()?;
    if first.is_empty() { None } else { Some(first.to_string()) }
}

/// Expand `%VAR%` the way a `REG_EXPAND_SZ` command line expects. `winreg`
/// hands back the literal string, so an unexpanded `%SystemRoot%` would be
/// parsed into a path that exists nowhere.
fn expand_env_vars(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(open) = rest.find('%') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('%') {
            Some(close) => {
                let name = &after[..close];
                match std::env::var(name) {
                    Ok(v) => out.push_str(&v),
                    // Unknown variable: keep it verbatim rather than deleting
                    // it, so a log reader sees what was not expanded.
                    Err(_) => {
                        out.push('%');
                        out.push_str(name);
                        out.push('%');
                    }
                }
                rest = &after[close + 1..];
            }
            None => {
                out.push('%');
                out.push_str(after);
                return out;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Who does Windows say handles `<scheme>://`, in a form an EXISTING matcher
/// can consume?
///
/// MEASURED ON THIS MACHINE, 2026-08-29 (`live_protocol_probe`), and the two
/// answers are genuinely different shapes:
///
/// ```text
/// whatsapp  HKCR\whatsapp            URL Protocol, and NO shell\open\command AT ALL
///           AssocQueryString APPID   5319275A.WhatsAppDesktop_cv1g1gvanyjgm!App
///           window process           WhatsApp.Root.exe   <- not "whatsapp"
/// discord   HKCR\discord\shell\open\command
///                                    "…\app-1.0.9255\Discord.exe" --url -- "%1"
///           AssocQueryString APPID   0x80070002 (none)
/// ```
///
/// So the Store shape has no command to parse and the classic shape has no
/// AUMID to read. Both routes are required; neither alone covers both.
///
/// MEMOISED FOR THE PROCESS LIFETIME. This is on the Space-hold path, and a
/// protocol registration changes only when an app is installed, uninstalled or
/// updated — none of which can happen without the user noticing, and all of
/// which are followed by a restart of the app sooner or later. The failure mode
/// of a stale entry is bounded and self-correcting: a stale exe path simply
/// fails to match any window and the binding launches, which is exactly today's
/// behaviour. Re-reading the registry on every keypress to catch an event that
/// happens a few times a year is the wrong trade.
#[cfg(windows)]
fn scheme_identities(scheme: &str) -> Vec<MatchIdentity> {
    use std::collections::HashMap;
    static MEMO: OnceLock<Mutex<HashMap<String, Vec<MatchIdentity>>>> = OnceLock::new();
    let memo = MEMO.get_or_init(|| Mutex::new(HashMap::new()));

    let key = scheme.to_lowercase();
    if key.is_empty() {
        return Vec::new();
    }
    if let Some(hit) = memo.lock().unwrap_or_else(|p| p.into_inner()).get(&key) {
        return hit.clone();
    }

    let mut ids = Vec::new();

    // Store/packaged handler first: it is the shape that has no other evidence.
    if let Some(aumid) = scheme_handler_aumid(&key) {
        log::info!(
            "cascade: {key}:// is handled by the PACKAGED app {aumid:?} — matching its \
             windows by AUMID/package family instead of by the binding's file name."
        );
        ids.push(MatchIdentity::Aumid(format!("shell:AppsFolder\\{aumid}")));
    }
    if let Some(exe) = scheme_handler_exe(&key) {
        let stem = launch_stem(&exe);
        if !stem.is_empty() {
            log::info!(
                "cascade: {key}:// is handled by {exe:?} — matching its windows by the exe \
                 stem {stem:?}."
            );
            ids.push(MatchIdentity::ExeStem(stem));
        }
    }
    if ids.is_empty() {
        log::info!(
            "cascade: nothing on this machine is registered to handle {key}:// in a form \
             we can match windows with (no AppUserModelID, no shell\\open\\command). This \
             binding will launch unconditionally, exactly as it did before."
        );
    }

    memo.lock().unwrap_or_else(|p| p.into_inner()).insert(key, ids.clone());
    ids
}

#[cfg(not(windows))]
fn scheme_identities(_scheme: &str) -> Vec<MatchIdentity> {
    Vec::new()
}

/// `HKEY_CLASSES_ROOT\<scheme>\shell\open\command`, the classic shape.
#[cfg(windows)]
fn scheme_handler_exe(scheme: &str) -> Option<String> {
    use winreg::enums::HKEY_CLASSES_ROOT;
    let hkcr = RegKey::predef(HKEY_CLASSES_ROOT);
    let key = hkcr.open_subkey(format!("{scheme}\\shell\\open\\command")).ok()?;
    let cmd: String = key.get_value("").ok()?;
    exe_from_shell_open_command(&expand_env_vars(&cmd))
}

/// `AssocQueryStringW(ASSOCF_IS_PROTOCOL, ASSOCSTR_APPID, …)` — the AUMID of
/// whatever handles this scheme.
///
/// This is the ONLY route that answers for a Store-packaged handler. The
/// registry does not: `HKCR\whatsapp` has a `URL Protocol` value and no
/// subkeys, and `HKCR\Extensions\ContractId\Windows.Protocol\PackageId` (48
/// packages on this machine — the enumeration was validated against them, so
/// its silence about WhatsApp is a real negative and not an empty read) has no
/// WhatsApp entry either. Modern packaged protocol registration lives in the
/// State Repository, which `AssocQueryString` reads and a registry walk cannot.
#[cfg(windows)]
fn scheme_handler_aumid(scheme: &str) -> Option<String> {
    use windows::core::{HSTRING, PCWSTR, PWSTR};
    use windows::Win32::UI::Shell::{AssocQueryStringW, ASSOCF_IS_PROTOCOL, ASSOCSTR_APPID};

    let assoc = HSTRING::from(scheme);
    let mut len: u32 = 512;
    let mut buf = vec![0u16; len as usize];
    let hr = unsafe {
        AssocQueryStringW(
            ASSOCF_IS_PROTOCOL,
            ASSOCSTR_APPID,
            PCWSTR(assoc.as_ptr()),
            PCWSTR::null(),
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        )
    };
    if hr.is_err() {
        return None;
    }
    let s = String::from_utf16_lossy(&buf[..(len as usize).saturating_sub(1)]);
    let s = s.trim_end_matches('\0').trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}

/// The stem of the program a binding string ACTUALLY starts — the launch's own
/// resolution, run once and remembered.
///
/// Memoised, and re-validated on every hit: if the remembered path has since
/// vanished the entry is dropped and re-resolved, so a self-updating app
/// (PROBLEM 116's class) cannot pin this to a folder that no longer exists.
#[cfg(windows)]
fn resolved_target_stem(target: &str) -> Option<String> {
    use std::collections::HashMap;
    static MEMO: OnceLock<Mutex<HashMap<String, Option<(String, String)>>>> = OnceLock::new();
    let memo = MEMO.get_or_init(|| Mutex::new(HashMap::new()));

    let key = target.to_lowercase();
    {
        let mut m = memo.lock().unwrap_or_else(|p| p.into_inner());
        match m.get(&key) {
            Some(Some((path, stem))) => {
                if std::path::Path::new(path).exists() {
                    return Some(stem.clone());
                }
                m.remove(&key); // the world changed — resolve again
            }
            Some(None) => return None,
            None => {}
        }
    }

    let path = if std::path::Path::new(target).is_absolute() {
        target.to_string()
    } else {
        match resolve_path(target) {
            Some(p) => p,
            None => {
                memo.lock().unwrap_or_else(|p| p.into_inner()).insert(key, None);
                return None;
            }
        }
    };

    // A `.lnk` names the shortcut, not the process. Ask the shell what it
    // points at, through the SAME System.Link.TargetParsingPath reader the
    // Apps-folder fallback already uses.
    let final_path = if std::path::Path::new(&path)
        .extension()
        .map(|x| x.eq_ignore_ascii_case("lnk"))
        .unwrap_or(false)
    {
        let _com = unsafe { ComGuard::new() };
        unsafe { apps_folder_target_path(&path) }.unwrap_or_else(|| path.clone())
    } else {
        path.clone()
    };

    let stem = launch_stem(&final_path);
    let out = if stem.is_empty() { None } else { Some((path, stem)) };
    let answer = out.as_ref().map(|(_, s)| s.clone());
    memo.lock().unwrap_or_else(|p| p.into_inner()).insert(key, out);
    answer
}

#[cfg(not(windows))]
fn resolved_target_stem(target: &str) -> Option<String> {
    resolve_path(target).map(|p| launch_stem(&p)).filter(|s| !s.is_empty())
}

/// Launch a target, then pull its window to the front (PROBLEM 170).
///
/// `launch_app_inner` is the original function unchanged; this wrapper exists
/// so the post-launch raise happens on EVERY successful launch route rather
/// than being remembered at each of the five `return shell_launch(...)` sites
/// inside it. A follow-up step that has to be repeated per return statement is
/// a step that gets missed the next time a route is added — the same reasoning
/// as PROBLEM 158.
///
/// The stem is derived from the ORIGINAL binding, not from whatever path
/// resolution produced: `Discord.lnk` and `…\app-1.0.9\Discord.exe` both stem
/// to `discord`, which is what the window's process is actually called.
///
/// `params` (added 2026-08-26) is the command line to pass, and is `None` for
/// every route that existed before browser profiles — so the ShellExecute call
/// those routes make is unchanged, argument for argument. Threading it through
/// as an extra parameter, rather than restructuring these two functions, was
/// deliberate: every call site in the crate was checked first (there are five,
/// all in this file, all reached from `smart_cascade`) and none of them needed
/// to change beyond passing the value along.
fn launch_app(
    exe_name: &str,
    params: Option<&str>,
    rule: &ProfileRule,
    app_handle: Option<tauri::AppHandle>,
) -> bool {
    // ONE resolution for the launch and for the raise (PROBLEM 216). The plan
    // is memoised where it is expensive, so asking for it twice in a press
    // costs a hash lookup.
    let plan = resolve_launch_plan(exe_name);
    let launched = launch_app_inner(exe_name, params, app_handle, &plan.shape);
    if launched {
        match &plan.raise {
            RaiseIdentity::ExeStems(stems) => raise_after_launch(stems.clone(), rule.clone()),
            RaiseIdentity::Resolved(name) => {
                // What the launch actually starts, not what the binding is
                // called. A Start-Menu `.lnk` is the case where those differ.
                let stem = resolved_target_stem(name).unwrap_or_else(|| launch_stem(name));
                raise_after_launch(vec![stem], rule.clone());
            }
            RaiseIdentity::Skip(why) => {
                log::info!(
                    "cascade: not watching for a window after launching {exe_name:?} — {why}."
                );
            }
        }
    }
    launched
}

/// The launch half. It dispatches on `TargetShape` AND ON NOTHING ELSE — that
/// is what keeps it in lockstep with `resolve_launch_plan`, which is the only
/// producer of a `TargetShape`. Adding a shape breaks both until the author has
/// written a launch action and a match identity for it.
fn launch_app_inner(
    exe_name: &str,
    params: Option<&str>,
    app_handle: Option<tauri::AppHandle>,
    shape: &TargetShape,
) -> bool {
    match shape {
        TargetShape::ShellVerb => {
            // Store/UWP app — hand the shell: path straight to ShellExecute,
            // which activates the package by AppUserModelID.
            //
            // `params` is not forwarded here: a shell:AppsFolder AUMID
            // activation is not a command line, and a Store activation is never
            // a Chromium browser exe, so there is nothing a --profile-directory
            // could apply to.
            log::info!("cascade: activating Store app: {exe_name}");
            shell_launch(exe_name, None, app_handle)
        }
        TargetShape::AbsolutePath => launch_absolute_path(exe_name, params, app_handle),
        TargetShape::ProtocolUri(uri) => {
            // `params` is not forwarded: a protocol URI (discord://) is an
            // activation, not a command line, and none of these targets is a
            // Chromium browser.
            log::info!("cascade: launching via URI protocol: {uri}");
            shell_launch(uri, None, app_handle)
        }
        TargetShape::BareName => launch_resolved_name(exe_name, params, app_handle),
    }
}

/// Case 1 of the old ladder, unchanged, lifted into its own function so
/// `launch_app_inner` is nothing but the shape dispatch.
fn launch_absolute_path(
    exe_name: &str,
    params: Option<&str>,
    app_handle: Option<tauri::AppHandle>,
) -> bool {
    let p = std::path::Path::new(exe_name);
    // Caller provided a full absolute path (e.g. from the settings
    // dashboard). ShellExecute handles .exe, .lnk (keeping the shortcut's
    // arguments — how Discord-style "Update.exe --processStart" launchers
    // work), and elevation-manifest exes.
    {
        if p.exists() {
            log::info!(
                "cascade: launching absolute path: {exe_name}{}",
                params.map(|a| format!(" {a}")).unwrap_or_default()
            );
            // THE route a browser+profile binding takes: the dashboard stores
            // an absolute exe path, so this is where --profile-directory lands.
            return shell_launch(exe_name, params, app_handle);
        } else {
            // PROBLEM 116 — a saved path that no longer exists is USUALLY not
            // an uninstalled app. It is an app that updated itself into a new
            // folder. Try to re-resolve before giving up.
            #[cfg(windows)]
            if let Some((target, repair_params)) = repair_versioned_path(p) {
                log::warn!(
                    "cascade: '{exe_name}' is gone — the app updated itself into a new \
                     folder. Re-resolved to '{target}{}'",
                    repair_params.as_deref().map(|a| format!(" {a}")).unwrap_or_default()
                );
                // The repair's OWN arguments win. A Squirrel launcher takes
                // `--processStart <exe>` and nothing else; appending a
                // --profile-directory to that would produce a command line
                // neither program understands. A Squirrel-packaged app is also
                // never a Chromium browser, so in practice `params` is always
                // None on this route — the check exists so a future caller gets
                // a log line instead of silence.
                if params.is_some() {
                    log::debug!(
                        "cascade: dropping extra launch parameters on the self-update \
                         repair route — the updater's own arguments take precedence"
                    );
                }
                return shell_launch(&target, repair_params.as_deref(), app_handle);
            }
            log::warn!("cascade: absolute path does not exist: {exe_name}");
            return false;
        }
    }
}

/// Cases 3 & 4 of the old ladder, unchanged: resolve from the known-app table,
/// App Paths, PATH and the Start Menu, then launch what that produced.
fn launch_resolved_name(
    exe_name: &str,
    params: Option<&str>,
    app_handle: Option<tauri::AppHandle>,
) -> bool {
    match resolve_path(exe_name) {
        Some(path) => {
            if !std::path::Path::new(&path).exists() {
                log::warn!("cascade: resolved path does not exist: {path}");
                return false;
            }
            log::info!(
                "cascade: launching {path}{}",
                params.map(|a| format!(" {a}")).unwrap_or_default()
            );
            // A bare exe NAME ("brave.exe") that resolved to a real path is
            // still a browser, so profile parameters apply here too.
            shell_launch(&path, params, app_handle)
        }
        None => {
            log::warn!("cascade: could not resolve path for {exe_name}");
            false
        }
    }
}


/// Run a URL in the preferred browser (Brave → Chrome → shell open).
/// Open a URL in the user's DEFAULT browser.
///
/// PROBLEM 60. This used to try brave.exe, then chrome.exe, and only then fall
/// back to a shell open — so on a tester's machine with neither installed, the
/// preferred paths missed and links reportedly opened the OneDrive Documents
/// FOLDER instead of a browser.
///
/// Two separate defects, both fixed here:
///
/// 1. HARDCODED BROWSERS. The user's choice of browser is Windows' business,
///    not ours. Deleted entirely.
///
/// 2. THE FOLDER-INSTEAD-OF-URL SYMPTOM. The old path went through
///    `shell_launch`, which uses `ShellExecuteExW` with `SEE_MASK_NOCLOSEPROCESS`
///    and calls `CoInitializeEx(COINIT_APARTMENTTHREADED)` on the ENGINE thread,
///    ignoring `RPC_E_CHANGED_MODE`. Ignoring that error means COM may already
///    be in a different apartment, and http protocol activation goes through
///    COM/DDE — when it fails, ShellExecute falls back to treating the argument
///    as a path relative to the process's CURRENT WORKING DIRECTORY. Launched by
///    the logon task, that cwd is the user profile, hence Documents/OneDrive.
///
///    Fixed by opening URLs on a DEDICATED thread that owns a clean STA, using
///    plain `ShellExecuteW` — the documented way to hand a URL to the default
///    browser — and by passing an explicit directory so a mis-parse can never
///    silently resolve against the cwd.
pub fn run_browser(url: &str, app_handle: Option<tauri::AppHandle>) -> bool {
    let url = url.trim().trim_matches('"').to_string();
    if url.is_empty() {
        log::warn!("cascade: refusing to open an empty URL");
        return false;
    }
    // Guard the exact failure above: anything without a scheme could be taken
    // as a file path. Give it one rather than letting the shell guess.
    let url = if url.contains("://") || url.starts_with("mailto:") {
        url
    } else {
        log::info!("cascade: URL {url:?} had no scheme — assuming https://");
        format!("https://{url}")
    };

    log::info!("cascade: opening {url} in the DEFAULT browser");

    #[cfg(windows)]
    {
        // PROBLEM 225 — this route uses `ShellExecuteW`, which returns no
        // process handle at all, so there is no PID to hand the watcher. Clear
        // it explicitly: `run_browser` is the ONE launch entry point that does
        // not go through `shell_launch`, so without this line it would inherit
        // whatever the previous launch left behind.
        LAST_LAUNCH_PID.store(0, std::sync::atomic::Ordering::Relaxed);

        let u = url.clone();
        // Dedicated thread: a fresh STA, so protocol activation cannot be
        // poisoned by whatever apartment the engine thread happens to be in.
        let joiner = std::thread::spawn(move || unsafe {
            use windows::core::HSTRING;
            use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
            use windows::Win32::UI::Shell::ShellExecuteW;
            use windows::Win32::UI::WindowsAndMessaging::{
                AllowSetForegroundWindow, ASFW_ANY, SW_SHOWNORMAL,
            };

            let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let file = HSTRING::from(u.as_str());
            let verb = HSTRING::from("open");
            // Explicit working directory — never let a mis-parse resolve the
            // URL against the process cwd (that is the Documents bug).
            let dir = HSTRING::from(std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into()));
            // PROBLEM 170 — see shell_launch. Immediately before the shell
            // call, so the browser we are about to start is allowed to bring
            // its own window forward instead of opening behind.
            let _ = AllowSetForegroundWindow(ASFW_ANY);
            let inst = ShellExecuteW(
                None,
                windows::core::PCWSTR(verb.as_ptr()),
                windows::core::PCWSTR(file.as_ptr()),
                windows::core::PCWSTR::null(),
                windows::core::PCWSTR(dir.as_ptr()),
                SW_SHOWNORMAL,
            );
            if hr.is_ok() {
                CoUninitialize();
            }
            // ShellExecuteW returns >32 on success.
            inst.0 as usize > 32
        });

        return match joiner.join() {
            Ok(true) => {
                if let Some(app) = &app_handle {
                    use tauri::Emitter;
                    let _ = app.emit("app-launched", &url);
                }
                // PROBLEM 170, half 2. The default browser is whatever the
                // user chose, so ask the registry rather than guessing —
                // `browser_stem` is the same lookup `url_focus_or_minimize`
                // uses to FIND browser windows, so the two agree by
                // construction. If it cannot be determined the raise is
                // skipped and the launch still stands.
                if let Some(stem) = browser_stem() {
                    // `ProfileRule::Any` and not a profile rule, deliberately:
                    // this is the DEFAULT-browser route, which launches with no
                    // `--profile-directory` at all, so any window of that
                    // browser is a legitimate thing to raise. Discriminating
                    // here would mean refusing to raise the very window we just
                    // opened.
                    raise_after_launch(vec![stem], ProfileRule::Any);
                }
                true
            }
            Ok(false) => {
                log::error!(
                    "cascade: ShellExecuteW refused {url} — no default browser is registered \
                     for http/https on this machine"
                );
                false
            }
            Err(_) => {
                log::error!("cascade: the URL-opening thread panicked for {url}");
                false
            }
        };
    }

    #[cfg(not(windows))]
    {
        let _ = app_handle;
        false
    }
}

fn open_with(exe: &str, url: &str, app_handle: Option<tauri::AppHandle>) -> bool {
    shell_launch(exe, Some(url), app_handle)
}

fn open_uri(uri: &str, app_handle: Option<tauri::AppHandle>) -> bool {
    let clean = uri.trim_matches('"');
    shell_launch(clean, None, app_handle)
}

/// PHASE A — the ONE launcher for `Action::Uri`: `ms-settings:…`,
/// `shell:…`, any `scheme:` URI, a folder, a document. The same
/// `ShellExecuteExW` path the cascade's `ShellVerb` and `ProtocolUri` shapes
/// take, exposed rather than duplicated in `actions/uri.rs`. `true` when the
/// shell accepted it.
pub fn open_target(target: &str, app_handle: Option<tauri::AppHandle>) -> bool {
    open_uri(target, app_handle)
}

/// Check for protocol-based URIs (discord://, spotify://, etc.)
fn protocol_uri(exe: &str) -> Option<String> {
    match exe.to_lowercase().as_str() {
        "discord.exe" => Some("discord://".into()),
        "spotify.exe" => Some("spotify://".into()),
        "whatsapp.exe" => Some("whatsapp://".into()),
        "steam.exe" => Some("steam://".into()),
        _ => None,
    }
}

/// Resolve an exe name to its full absolute path.
/// Mirrors V11 ResolvePath() exactly with the same priority ordering.
pub fn resolve_path(exe_name: &str) -> Option<String> {
    let p = std::env::var("ProgramFiles").unwrap_or_default();
    let p86 = std::env::var("ProgramFiles(x86)").unwrap_or_default();
    let l = std::env::var("LOCALAPPDATA").unwrap_or_default();
    let a = std::env::var("APPDATA").unwrap_or_default();

    let candidates: Vec<String> = match exe_name.to_lowercase().as_str() {
        "brave.exe" => vec![
            format!("{p}\\BraveSoftware\\Brave-Browser\\Application\\brave.exe"),
            format!("{l}\\BraveSoftware\\Brave-Browser\\Application\\brave.exe"),
        ],
        "chrome.exe" => vec![
            format!("{p}\\Google\\Chrome\\Application\\chrome.exe"),
            format!("{l}\\Google\\Chrome\\Application\\chrome.exe"),
        ],
        "obs64.exe" => vec![
            format!("{p}\\obs-studio\\bin\\64bit\\obs64.exe"),
            format!("{p86}\\obs-studio\\bin\\64bit\\obs64.exe"),
        ],
        "excel.exe" => vec![
            format!("{p}\\Microsoft Office\\root\\Office16\\EXCEL.EXE"),
            format!("{p86}\\Microsoft Office\\root\\Office16\\EXCEL.EXE"),
        ],
        "powerpnt.exe" => vec![
            format!("{p}\\Microsoft Office\\root\\Office16\\POWERPNT.EXE"),
        ],
        "outlook.exe" => vec![
            format!("{p}\\Microsoft Office\\root\\Office16\\OUTLOOK.EXE"),
        ],
        "photoshop.exe" => {
            // Dynamic search for all Adobe Photoshop version folders
            let mut v = vec![
                format!("{p}\\Adobe\\Adobe Photoshop 2026\\Photoshop.exe"),
                format!("{p}\\Adobe\\Adobe Photoshop 2025\\Photoshop.exe"),
                format!("{p}\\Adobe\\Adobe Photoshop 2024\\Photoshop.exe"),
            ];
            // Try glob-style search via read_dir
            if let Ok(entries) = std::fs::read_dir(format!("{p}\\Adobe")) {
                for e in entries.flatten() {
                    let name = e.file_name().to_string_lossy().to_lowercase();
                    if name.starts_with("adobe photoshop") {
                        v.push(format!("{}\\Photoshop.exe", e.path().display()));
                    }
                }
            }
            v
        }
        "leagueclient.exe" => vec![
            "C:\\Riot Games\\League of Legends\\LeagueClient.exe".into(),
        ],
        "epicgameslauncher.exe" => vec![
            format!("{p86}\\Epic Games\\Launcher\\Portal\\Binaries\\Win64\\EpicGamesLauncher.exe"),
            format!("{p}\\Epic Games\\Launcher\\Portal\\Binaries\\Win64\\EpicGamesLauncher.exe"),
        ],
        "blender.exe" => vec![
            format!("{p}\\Blender Foundation\\Blender\\blender.exe"),
        ],
        "canva.exe" => vec![
            format!("{l}\\Programs\\Canva\\Canva.exe"),
        ],
        "resolve.exe" => vec![
            format!("{p}\\Blackmagic Design\\DaVinci Resolve\\Resolve.exe"),
        ],
        "slack.exe" => vec![
            format!("{l}\\Programs\\slack\\slack.exe"),
        ],
        "telegram.exe" => vec![
            format!("{a}\\Telegram Desktop\\Telegram.exe"),
        ],
        "utorrent.exe" => vec![
            format!("{a}\\uTorrent\\uTorrent.exe"),
        ],
        "vlc.exe" => vec![
            format!("{p}\\VideoLAN\\VLC\\vlc.exe"),
            format!("{p86}\\VideoLAN\\VLC\\vlc.exe"),
        ],
        "zoom.exe" => vec![
            format!("{a}\\Zoom\\bin\\Zoom.exe"),
        ],
        "notepad.exe" => vec![
            "C:\\Windows\\System32\\notepad.exe".into(),
        ],
        "notion.exe" => vec![
            format!("{l}\\Programs\\Notion\\Notion.exe"),
        ],
        "wt.exe" => vec![
            format!("{l}\\Microsoft\\WindowsApps\\wt.exe"),
        ],
        "radeon software.exe" | "radeonsoftware.exe" => vec![
            format!("{p}\\AMD\\CNext\\CNext\\RadeonSoftware.exe"),
        ],
        "msiafterburner.exe" => vec![
            format!("{p86}\\MSI Afterburner\\MSIAfterburner.exe"),
        ],
        "explorer.exe" => vec![
            "C:\\Windows\\explorer.exe".into(),
        ],
        _ => vec![],
    };

    // Check each candidate
    for c in &candidates {
        if std::path::Path::new(c).exists() {
            return Some(c.clone());
        }
    }

    // Registry fallback: HKLM then HKCU App Paths
    #[cfg(windows)]
    {
        let reg_key = format!("SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\App Paths\\{exe_name}");
        for hive in [
            RegKey::predef(HKEY_LOCAL_MACHINE),
            RegKey::predef(HKEY_CURRENT_USER),
        ] {
            if let Ok(key) = hive.open_subkey(&reg_key) {
                if let Ok(path) = key.get_value::<String, _>("") {
                    if std::path::Path::new(&path).exists() {
                        return Some(path);
                    }
                }
            }
        }
    }

    // ---------------------------------------------------------------------
    // PATH search — catches anything on the system PATH (wt.exe, git, etc.)
    // ---------------------------------------------------------------------
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in path_var.split(';').filter(|d| !d.is_empty()) {
            let cand = std::path::Path::new(dir).join(exe_name);
            if cand.exists() {
                return Some(cand.to_string_lossy().into_owned());
            }
        }
    }

    // ---------------------------------------------------------------------
    // START MENU SHORTCUT SEARCH — the fallback that actually finds things.
    //
    // PROBLEM 53. Everything above only works for apps whose exact install
    // path is hardcoded in the table, or which register an App Paths key, or
    // which sit on PATH. A tester's log showed the consequence plainly:
    //   cascade: could not resolve path for Battle.net.exe
    //   cascade: could not resolve path for NVIDIA GeForce Experience.exe
    //   cascade: could not resolve path for HaloInfinite.exe
    // …so Space+key did nothing for most of his bindings, even though the
    // hook and engine were working perfectly. Nearly every installed Windows
    // app puts a .lnk in one of the two Start Menu trees, which is exactly
    // where the app picker already finds them — so resolve here from the same
    // source. ShellExecute launches a .lnk fine (the picker already stores
    // .lnk paths for apps whose shortcut carries arguments).
    // ---------------------------------------------------------------------
    let stem = std::path::Path::new(exe_name)
        .file_stem()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if !stem.is_empty() {
        let roots = [
            std::env::var("APPDATA")
                .map(|v| format!("{v}\\Microsoft\\Windows\\Start Menu\\Programs"))
                .unwrap_or_default(),
            std::env::var("ProgramData")
                .map(|v| format!("{v}\\Microsoft\\Windows\\Start Menu\\Programs"))
                .unwrap_or_default(),
        ];
        for root in roots.iter().filter(|r| !r.is_empty()) {
            if let Some(hit) = find_shortcut(std::path::Path::new(root), &stem, 0) {
                log::info!("cascade: resolved {exe_name:?} via Start Menu → {hit}");
                return Some(hit);
            }
        }
    }

    log::warn!(
        "cascade: could not resolve {exe_name:?} — not in the known-paths table, not in \
         App Paths, not on PATH, and no matching Start Menu shortcut. It is probably not \
         installed, or was installed without a Start Menu entry."
    );
    None
}

/// Recursively look for a `.lnk` whose file name matches `stem`.
/// Depth-capped: Start Menu trees are shallow, and an unbounded walk on a
/// keypress path is not acceptable.
#[cfg(windows)]
fn find_shortcut(dir: &std::path::Path, stem: &str, depth: u32) -> Option<String> {
    if depth > 3 {
        return None;
    }
    let entries = std::fs::read_dir(dir).ok()?;
    let mut subdirs = Vec::new();
    for e in entries.flatten() {
        let path = e.path();
        if path.is_dir() {
            subdirs.push(path);
            continue;
        }
        if path.extension().and_then(|x| x.to_str()).map(|x| x.eq_ignore_ascii_case("lnk"))
            != Some(true)
        {
            continue;
        }
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        // Exact first, then a contains-match so "NVIDIA app" finds
        // "NVIDIA App.lnk" and "Battle.net" finds "Battle.net Launcher.lnk".
        if name == stem || name.starts_with(stem) || stem.starts_with(&name) {
            return Some(path.to_string_lossy().into_owned());
        }
    }
    for d in subdirs {
        if let Some(hit) = find_shortcut(&d, stem, depth + 1) {
            return Some(hit);
        }
    }
    None
}

#[cfg(not(windows))]
fn find_shortcut(_d: &std::path::Path, _s: &str, _depth: u32) -> Option<String> { None }

/* ===========================================================================
   TESTS — PROBLEM 116, the self-updating-app path repair.
   ===========================================================================
   The first automated tests in this project, added deliberately and only here.
   The reason is PROBLEM 118: 1.0.33 shipped a recovery branch that had never
   once been executed, and it was wrong in two ways that a single run would
   have caught. This repair is the same shape — a branch that only fires when
   something has already gone wrong, so it is exactly the code least likely to
   be exercised before a user hits it.

   It also could not be tested on the developer's machine by hand: Discord was
   open during every attempt, so `smart_cascade` matched the running window by
   executable name and the launch path never ran at all. These tests build the
   Squirrel folder layout in a temp directory and check the resolution directly,
   which needs no application installed and works on any machine.
   =========================================================================== */
#[cfg(all(test, windows))]
mod repair_tests {
    use super::repair_versioned_path;
    use std::fs;
    use std::path::{Path, PathBuf};

    /// A scratch directory that removes itself, so a failed test cannot leave
    /// litter behind that makes the NEXT run pass for the wrong reason.
    struct Scratch(PathBuf);
    impl Scratch {
        fn new(tag: &str) -> Self {
            let mut p = std::env::temp_dir();
            p.push(format!("spaceadom-test-{tag}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&p);
            fs::create_dir_all(&p).expect("scratch dir");
            Scratch(p)
        }
        fn path(&self) -> &Path { &self.0 }
    }
    impl Drop for Scratch {
        fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); }
    }

    fn touch(p: &Path) {
        if let Some(d) = p.parent() { fs::create_dir_all(d).unwrap(); }
        fs::write(p, b"x").unwrap();
    }

    /// The real case: Discord updated 9251 -> 9253 and the saved path died.
    #[test]
    fn resolves_to_the_newer_version_folder() {
        let s = Scratch::new("newer");
        let app = s.path().join("Discord");
        let live = app.join("app-1.0.9253").join("Discord.exe");
        touch(&live);

        let dead = app.join("app-1.0.9251").join("Discord.exe");
        let (target, params) = repair_versioned_path(&dead).expect("should re-resolve");

        assert_eq!(Path::new(&target), live.as_path());
        assert!(params.is_none(), "a direct exe needs no arguments");
    }

    /// Version strings stop sorting lexicographically once a component reaches
    /// double digits: "app-1.0.10" sorts BEFORE "app-1.0.9" as text. Choosing
    /// by modification time is what makes this correct, and this test is here
    /// to stop anyone "simplifying" it back to a name sort.
    #[test]
    fn picks_by_modification_time_not_by_name() {
        let s = Scratch::new("sorting");
        let app = s.path().join("Slack");

        let older = app.join("app-1.0.9").join("Slack.exe");
        touch(&older);
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let newer = app.join("app-1.0.10").join("Slack.exe");   // sorts EARLIER as text
        touch(&newer);

        let dead = app.join("app-1.0.8").join("Slack.exe");
        let (target, _) = repair_versioned_path(&dead).expect("should re-resolve");
        assert_eq!(
            Path::new(&target), newer.as_path(),
            "must choose the most recently written folder, not the alphabetically last"
        );
    }

    /// When no usable version folder remains, fall back to Squirrel's own
    /// launcher — the stable entry point that survives every future update.
    #[test]
    fn falls_back_to_the_squirrel_updater() {
        let s = Scratch::new("updater");
        let app = s.path().join("Teams");
        touch(&app.join("Update.exe"));

        let dead = app.join("app-1.0.1").join("Teams.exe");
        let (target, params) = repair_versioned_path(&dead).expect("should fall back");

        assert_eq!(Path::new(&target), app.join("Update.exe").as_path());
        assert_eq!(params.as_deref(), Some("--processStart Teams.exe"));
    }

    /// A version folder with the WRONG executable inside must not be accepted;
    /// the updater is the correct answer there.
    #[test]
    fn does_not_accept_a_version_folder_missing_the_exe() {
        let s = Scratch::new("wrongexe");
        let app = s.path().join("Signal");
        touch(&app.join("app-2.0.0").join("SomethingElse.exe"));
        touch(&app.join("Update.exe"));

        let dead = app.join("app-1.0.0").join("Signal.exe");
        let (target, params) = repair_versioned_path(&dead).expect("should fall back");
        assert_eq!(Path::new(&target), app.join("Update.exe").as_path());
        assert_eq!(params.as_deref(), Some("--processStart Signal.exe"));
    }

    /// Genuinely uninstalled: nothing to offer, and the caller must get None so
    /// it logs the real error instead of launching something arbitrary.
    #[test]
    fn returns_none_when_the_app_is_really_gone() {
        let s = Scratch::new("gone");
        let dead = s.path().join("Ghost").join("app-1.0.0").join("Ghost.exe");
        assert!(repair_versioned_path(&dead).is_none());
    }

    /// An ordinary program in Program Files has no `app-<version>` ancestor and
    /// must be left completely alone — this repair must never fire on a path it
    /// does not understand.
    #[test]
    fn ignores_paths_that_are_not_squirrel_shaped() {
        let s = Scratch::new("plain");
        let exe = s.path().join("Notepad++").join("notepad++.exe");
        touch(&exe);
        let dead = s.path().join("Notepad++").join("gone.exe");
        assert!(repair_versioned_path(&dead).is_none());
    }
}

/* ===========================================================================
   TESTS — binding identity, 2026-08-27.

   The reported bug had two halves and BOTH are here, because either one alone
   still reproduces it: a cache that cannot tell two bindings apart, and a
   matcher that cannot tell two windows apart. Neither half is testable by
   hand on this machine in any repeatable way — it needs two profiles of one
   browser open in a particular order — and the incident that started this was
   caught only because the owner happened to be looking at his log.

   Every branch below is one a user reaches only after something has already
   gone ambiguous, which is the exact class CLAUDE.md says to cover. A wrong
   answer here does not crash and does not warn; it minimises somebody else's
   window.
   =========================================================================== */
#[cfg(test)]
mod identity_tests {
    use super::*;
    use crate::config::KeyBinding;

    const BRAVE_EXE: &str =
        r"C:\Program Files\BraveSoftware\Brave-Browser\Application\brave.exe";

    fn brave(profile: Option<&str>) -> KeyBinding {
        KeyBinding {
            app: Some(BRAVE_EXE.into()),
            browser_profile_dir: profile.map(str::to_string),
            ..Default::default()
        }
    }

    /// Nothing on disk at all. `user_data_dir_for` then locates no `User Data`
    /// directory, and `profile_arg_for`'s deliberate polarity — "could not
    /// verify" must never be treated as "verified absent" — keeps the pin. So
    /// this is the fixture for "the profile is fine", and it proves at the same
    /// time that a pinned binding does not quietly depend on a browser layout
    /// this code happens to recognise.
    fn nothing_exists(_: &std::path::Path) -> bool {
        false
    }

    /// The browser's `User Data` directory is right there and the pinned
    /// profile folder inside it is NOT: the user deleted that profile from
    /// inside the browser. The one case `profile_arg_for` calls
    /// `DroppedStale`.
    fn profile_folder_deleted(p: &std::path::Path) -> bool {
        p.file_name().is_some_and(|n| n == "User Data")
    }

    // ------------------------------------------------------------------ TASK 1
    // The cache key. This is the half that ACTUALLY fired the wrong minimise at
    // 23:44:10 — even a perfect matcher is defeated by a cache keyed on
    // something two bindings share.

    #[test]
    fn two_pinned_bindings_on_one_browser_get_different_cache_keys() {
        let b = ProfileRule::Pinned("Profile 1".into());
        let n = ProfileRule::Pinned("Profile 2".into());
        assert_ne!(
            cache_key("brave", &b),
            cache_key("brave", &n),
            "this equality IS the reported bug: one key, one HWND, one window \
             minimised for a key that was never aimed at it"
        );
    }

    #[test]
    fn a_pinned_and_an_unpinned_binding_do_not_share_a_cache_key() {
        assert_ne!(
            cache_key("brave", &ProfileRule::Any),
            cache_key("brave", &ProfileRule::Pinned("Profile 1".into()))
        );
        assert_ne!(
            cache_key("brave", &ProfileRule::Unpinned(vec!["Profile 1".into()])),
            cache_key("brave", &ProfileRule::Pinned("Profile 1".into()))
        );
    }

    /// The same profile spelled the two ways the two Windows properties spell
    /// it must be ONE cache entry, not two entries pointing at one window.
    /// Shares its normaliser with `same_profile_dir` so the key and the window
    /// comparison cannot drift.
    #[test]
    fn one_profile_spelled_two_ways_is_one_cache_entry() {
        assert_eq!(
            cache_key("brave", &ProfileRule::Pinned("Profile 1".into())),
            cache_key("brave", &ProfileRule::Pinned("profile1".into()))
        );
    }

    #[test]
    fn different_browsers_never_share_a_key() {
        let p = ProfileRule::Pinned("Profile 1".into());
        assert_ne!(cache_key("brave", &p), cache_key("chrome", &p));
        assert_ne!(
            cache_key("brave", &ProfileRule::Any),
            cache_key("chrome", &ProfileRule::Any)
        );
    }

    /// `|` cannot appear in a Windows file name, so the unpinned marker can
    /// never be produced by any real exe stem or profile folder.
    #[test]
    fn the_unpinned_marker_is_explicit_and_cannot_be_forged() {
        assert_eq!(cache_key("brave", &ProfileRule::Any), "brave|*");
        assert_eq!(
            cache_key("brave", &ProfileRule::Pinned("Profile 1".into())),
            "brave|profile1"
        );
    }

    /// Every binding that has nothing to do with browser profiles must still
    /// get a stable key of its own — the cache is what makes the cascade cyclic
    /// (CORE_AIM "Cyclic Reliability"), and a key that changed between presses
    /// would silently disable it.
    #[test]
    fn an_ordinary_app_binding_gets_one_stable_key() {
        assert_eq!(
            cache_key("discord", &ProfileRule::Any),
            cache_key("discord", &ProfileRule::Any)
        );
    }

    // ------------------------------------------------------------------ RULES

    #[test]
    fn a_pinned_binding_demands_its_own_profile_and_ignores_claims() {
        let claims = vec![("brave".to_string(), "Profile 2".to_string())];
        assert_eq!(
            rule_for_binding(&brave(Some("Profile 1")), BRAVE_EXE, &claims, &nothing_exists),
            ProfileRule::Pinned("Profile 1".into())
        );
    }

    /// THE PROPERTY THAT MAKES THIS SAFE TO SHIP: with nothing pinned anywhere,
    /// every binding is `Any`, and `Any` reads no window properties, opens no
    /// COM apartment and behaves exactly as the cascade did before this change.
    #[test]
    fn with_nothing_pinned_every_binding_is_unchanged() {
        assert_eq!(
            rule_for_binding(&brave(None), BRAVE_EXE, &[], &nothing_exists),
            ProfileRule::Any
        );
        let notepad = KeyBinding {
            app: Some("notepad.exe".into()),
            ..Default::default()
        };
        assert_eq!(
            rule_for_binding(&notepad, "notepad.exe", &[], &nothing_exists),
            ProfileRule::Any
        );
    }

    #[test]
    fn a_claim_on_another_browser_leaves_this_binding_alone() {
        let claims = vec![("chrome".to_string(), "Profile 6".to_string())];
        assert_eq!(
            rule_for_binding(&brave(None), BRAVE_EXE, &claims, &nothing_exists),
            ProfileRule::Any,
            "Chrome's pin must not make an unpinned BRAVE binding start declining \
             Brave windows"
        );
    }

    #[test]
    fn an_unpinned_binding_collects_only_this_browsers_claims() {
        let claims = vec![
            ("brave".to_string(), "Profile 1".to_string()),
            ("chrome".to_string(), "Profile 6".to_string()),
            ("brave".to_string(), "Profile 4".to_string()),
        ];
        assert_eq!(
            rule_for_binding(&brave(None), BRAVE_EXE, &claims, &nothing_exists),
            ProfileRule::Unpinned(vec!["Profile 1".into(), "Profile 4".into()])
        );
    }

    // ------------------------------------------------- REVIEW FINDING, 08-27
    // A PIN WHOSE FOLDER HAS BEEN DELETED.
    //
    // The launch leg drops `--profile-directory` and opens the browser's own
    // default profile (`profile_arg_for` -> `DroppedStale`, and
    // `launch_binding_app` downgrades its raise to match). The match leg used
    // to keep demanding the dead profile, so no living window could ever
    // satisfy it: press 1 launched, and press 2, 3, 4 … launched AGAIN, each
    // with another "that profile is gone" toast, and the key never
    // focus-then-minimised again. Both legs now ask the one decision function.

    #[test]
    fn a_pin_whose_folder_was_deleted_stops_demanding_it() {
        assert_eq!(
            rule_for_binding(
                &brave(Some("Profile 9")),
                BRAVE_EXE,
                &[],
                &profile_folder_deleted
            ),
            ProfileRule::Any,
            "a profile the launch leg has already given up on must not still be \
             demanded here — that is a key that can never toggle again"
        );
    }

    /// …and it degrades to UNPINNED, not to a free-for-all: the owner's rule
    /// still protects another binding's window. This is the half that keeps the
    /// original incident fixed while the stale case is being repaired.
    #[test]
    fn a_stale_pin_degrades_to_unpinned_not_to_anything_goes() {
        let claims = vec![("brave".to_string(), "Profile 1".to_string())];
        assert_eq!(
            rule_for_binding(
                &brave(Some("Profile 9")),
                BRAVE_EXE,
                &claims,
                &profile_folder_deleted
            ),
            ProfileRule::Unpinned(vec!["Profile 1".into()]),
            "a binding whose own pin died must behave as an unpinned one — free to \
             take an unclaimed window, still forbidden Space+N's"
        );
    }

    /// The polarity that must NOT change while fixing the stale case: a profile
    /// we could not verify (no derivable `User Data` — Arc's MSIX alias) is
    /// still pinned. "Could not check" is not "confirmed gone", and treating it
    /// as gone would un-pin every binding on a browser laid out in a way this
    /// code has not met.
    #[test]
    fn an_unverifiable_profile_stays_pinned() {
        assert_eq!(
            rule_for_binding(&brave(Some("Profile 1")), BRAVE_EXE, &[], &nothing_exists),
            ProfileRule::Pinned("Profile 1".into())
        );
    }

    /* ---------------------------------------------------------------- URL LEG
       THE REVIEW'S MAJOR FINDING, 2026-08-27 — and the reason these tests exist
       at the rule level rather than at the window level.

       The first draft of the URL matcher applied the app leg's claims rule on
       all three of its routes. Two of those routes launch through
       `run_browser`, which passes NO `--profile-directory` at all, so the match
       leg could refuse the exact window the launch would land in — and
       `url_focus_or_minimize` returning false does not mean "do nothing", it
       means "open the URL". On the owner's own machine (Brave is both the
       default browser and the one he pins profiles of) a plain youtube.com key
       that pinned nothing whatsoever would have opened a duplicate tab on every
       single press, forever, with the minimise half unreachable. That is the
       precise bug this function was written on 2026-08-11 to prevent.

       Nothing here touches a window: `url_rule_for_route` is the whole
       decision, so the whole decision is testable.
       ======================================================================= */

    const CHROME_EXE: &str = r"C:\Program Files\Google\Chrome\Application\chrome.exe";

    fn url_binding(browser: Option<&str>, profile: Option<&str>) -> KeyBinding {
        KeyBinding {
            web_url: Some("https://youtube.com".into()),
            browser_exe: browser.map(str::to_string),
            browser_profile_dir: profile.map(str::to_string),
            ..Default::default()
        }
    }

    /// The browser is installed and its profile folder is where it should be.
    fn everything_exists(_: &std::path::Path) -> bool {
        true
    }

    /// The browser is installed, its `User Data` is there, and the pinned
    /// profile folder inside it has been deleted from within the browser.
    fn browser_installed_profile_deleted(p: &std::path::Path) -> bool {
        p.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe"))
            || p.file_name().is_some_and(|n| n == "User Data")
    }

    fn url_rule(binding: &KeyBinding, exists: &dyn Fn(&std::path::Path) -> bool) -> ProfileRule {
        let route = crate::browser_profiles::route_for(binding, exists);
        url_rule_for_route(&route, binding, exists)
    }

    /// THE OWNER'S SCENARIO, from the review, at the rule level. Space+Y is
    /// `youtube.com` with no browser pinned; Space+N pins Brave "Profile 1".
    /// Space+Y must go on toggling his YouTube window — it is not Space+N's
    /// window that it is claiming, it is the window showing its own site.
    #[test]
    fn a_url_that_pins_nothing_is_never_narrowed_by_another_keys_pin() {
        assert_eq!(
            url_rule(&url_binding(None, None), &everything_exists),
            ProfileRule::Any,
            "a URL binding the user never pinned anything on must match exactly \
             what it matched in 1.0.86 — its launch leg is run_browser, which \
             cannot target a profile, so a narrower match can only ever open a \
             duplicate tab"
        );
    }

    /// The same key, with a profile somehow set but no browser. Both legs
    /// ignore it: `run_browser` has nowhere to put it.
    #[test]
    fn a_profile_without_a_browser_is_ignored_on_the_match_leg_too() {
        assert_eq!(
            url_rule(&url_binding(None, Some("Profile 1")), &everything_exists),
            ProfileRule::Any
        );
    }

    /// Pinned browser AND profile, both present: this is the one case where the
    /// launch passes `--profile-directory`, so it is the one case the match leg
    /// may demand a profile.
    #[test]
    fn a_url_pinned_to_a_live_profile_demands_that_profile() {
        assert_eq!(
            url_rule(&url_binding(Some(CHROME_EXE), Some("Profile 6")), &everything_exists),
            ProfileRule::Pinned("Profile 6".into())
        );
    }

    /// Pinned browser, no profile. The launch passes only the URL, so the tab
    /// lands in whichever profile Chrome used last — which is unknowable here.
    #[test]
    fn a_url_pinned_to_a_browser_but_no_profile_takes_any_of_its_windows() {
        assert_eq!(
            url_rule(&url_binding(Some(CHROME_EXE), None), &everything_exists),
            ProfileRule::Any
        );
    }

    /// The stale case on the URL leg. Its cost is worse in kind than the app
    /// leg's: a match that can never succeed does not merely relaunch, it opens
    /// another tab, and they pile up one per press.
    #[test]
    fn a_url_whose_pinned_profile_was_deleted_stops_demanding_it() {
        assert_eq!(
            url_rule(
                &url_binding(Some(CHROME_EXE), Some("Profile 6")),
                &browser_installed_profile_deleted
            ),
            ProfileRule::Any,
            "the launch leg has already dropped this profile; a match leg still \
             demanding it is a key that opens a duplicate tab on every press"
        );
    }

    /// The pinned browser has been uninstalled. Both legs fall back to the
    /// default browser and drop the profile with it.
    #[test]
    fn an_uninstalled_pinned_browser_drops_the_profile_demand_as_well() {
        assert_eq!(
            url_rule(&url_binding(Some(CHROME_EXE), Some("Profile 6")), &nothing_exists),
            ProfileRule::Any
        );
    }

    /// THE INVARIANT ITSELF, held across every combination rather than one case
    /// at a time: **the match leg demands a profile exactly when the launch leg
    /// passes one, and it is the same profile.** This is the property whose
    /// violation produced the finding; asserting the cases individually would
    /// not have caught a fourth arm being added later.
    #[test]
    fn the_url_match_leg_demands_exactly_what_the_launch_leg_passes() {
        use crate::browser_profiles::{self, BrowserRoute, ProfileArg};

        let cases: Vec<(&str, KeyBinding, &dyn Fn(&std::path::Path) -> bool)> = vec![
            ("no browser, no profile", url_binding(None, None), &everything_exists),
            ("no browser, stray profile", url_binding(None, Some("Profile 1")), &everything_exists),
            ("browser only", url_binding(Some(CHROME_EXE), None), &everything_exists),
            (
                "browser + live profile",
                url_binding(Some(CHROME_EXE), Some("Profile 6")),
                &everything_exists,
            ),
            (
                "browser + deleted profile",
                url_binding(Some(CHROME_EXE), Some("Profile 6")),
                &browser_installed_profile_deleted,
            ),
            (
                "uninstalled browser",
                url_binding(Some(CHROME_EXE), Some("Profile 6")),
                &nothing_exists,
            ),
        ];

        for (name, binding, exists) in cases {
            let route = browser_profiles::route_for(&binding, exists);
            let rule = url_rule_for_route(&route, &binding, exists);

            // What `open_binding_url` will actually put on the command line for
            // this same binding, derived the way that function derives it.
            let launch_profile = match &route {
                BrowserRoute::Specific { exe } => match browser_profiles::profile_arg_for(
                    exe,
                    binding.browser_profile_dir.as_deref(),
                    browser_profiles::local_app_data().as_deref(),
                    exists,
                ) {
                    ProfileArg::Use(d) => Some(d),
                    ProfileArg::None | ProfileArg::DroppedStale { .. } => None,
                },
                // Both fall back to `run_browser`, which takes a URL and
                // nothing else.
                BrowserRoute::Default | BrowserRoute::PinnedBrowserMissing { .. } => None,
            };
            let params = browser_profiles::build_launch_params(
                launch_profile.as_deref(),
                Some("https://youtube.com"),
            )
            .unwrap_or_default();

            match (&rule, launch_profile.as_deref()) {
                (ProfileRule::Pinned(want), Some(passed)) => {
                    assert_eq!(want, passed, "{name}: matched and launched profiles differ");
                    assert!(
                        params.contains(&format!("--profile-directory=\"{passed}\"")),
                        "{name}: the launch does not actually carry that switch"
                    );
                }
                (ProfileRule::Any, None) => {
                    assert!(
                        !params.contains("--profile-directory"),
                        "{name}: the match leg gave up discriminating but the launch \
                         still pins a profile"
                    );
                }
                other => panic!(
                    "{name}: match and launch disagree — {other:?}. Every URL route \
                     must either demand the profile it is about to pass, or demand \
                     nothing because it is about to pass nothing."
                ),
            }
        }
    }

    /* ------------------------------------------------------- THE RAISE RULE
       Review finding, 2026-08-27: the URL leg's post-launch raise used
       `claims_rule` for BOTH of its no-profile outcomes, so after a stale pin
       was dropped it could refuse to raise the window the launch had just
       created — and the reviewer's other point was fair too, that the branch
       "lives inline in open_binding_url rather than in a pure, testable
       function". It does not any more, and both legs go through it.
       ======================================================================= */

    /// A launch that NAMED a profile is followed by a raise that demands it.
    #[test]
    fn a_launch_that_passed_a_profile_raises_only_that_profile() {
        assert_eq!(
            raise_rule_for_launch(Some("Profile 6"), false, &ProfileRule::Any),
            ProfileRule::Pinned("Profile 6".into())
        );
    }

    /// THE FINDING. The pin was dropped because its folder is gone, so the
    /// browser opened its own default profile — which may well be one another
    /// binding has claimed. Demanding an unclaimed profile here means the
    /// watcher polls for eight seconds and raises nothing while the window it
    /// wanted is on screen.
    #[test]
    fn a_dropped_stale_pin_raises_whatever_the_browser_actually_opened() {
        let claimed_by_someone_else = ProfileRule::Unpinned(vec!["Default".into()]);
        assert_eq!(
            raise_rule_for_launch(None, true, &claimed_by_someone_else),
            ProfileRule::Any,
            "the launch dropped the pin, so the raise must too — or it refuses to \
             raise the window it just caused to exist"
        );
    }

    /// And the case that must NOT be widened along with it: a binding that
    /// pinned nothing in the first place keeps its own rule, so an unpinned key
    /// that just launched still does not pull another binding's window forward.
    #[test]
    fn a_binding_that_pinned_nothing_keeps_its_own_rule_on_the_raise() {
        let mine = ProfileRule::Unpinned(vec!["Profile 1".into()]);
        assert_eq!(raise_rule_for_launch(None, false, &mine), mine);
        assert_eq!(
            raise_rule_for_launch(None, false, &ProfileRule::Any),
            ProfileRule::Any
        );
    }

    /// The app leg's plan and its raise must not drift: whatever
    /// `app_launch_plan` decides to put on the command line is what the raise
    /// asks for. Asserted through the REAL plan builder, which is the half a
    /// pure-function test cannot reach.
    ///
    /// The exe is a path that exists on no machine, deliberately.
    /// `app_launch_plan` reads the real filesystem (it is the launch leg, so it
    /// must), and pointing it at a browser that is really installed would make
    /// this test's result depend on which profiles the person running it
    /// happens to have. An unresolvable product folder takes
    /// `profile_arg_for`'s documented "could not verify, so leave it alone"
    /// branch, which is deterministic everywhere.
    #[test]
    fn the_app_legs_raise_follows_its_own_launch_plan() {
        const GHOST_EXE: &str = r"C:\NoSuchVendor\NoSuchBrowser\Application\ghost.exe";
        let pinned = KeyBinding {
            app: Some(GHOST_EXE.into()),
            browser_profile_dir: Some("Profile 1".into()),
            ..Default::default()
        };

        // A pin the launch keeps: the params carry the switch, and the raise
        // demands exactly it.
        let plan = app_launch_plan(&pinned);
        assert!(
            plan.params
                .as_deref()
                .unwrap_or_default()
                .contains("--profile-directory=\"Profile 1\""),
            "the plan must actually pass the switch for this assertion to mean anything"
        );
        let rule = rule_for_binding(&pinned, GHOST_EXE, &[], &nothing_exists);
        assert_eq!(
            raise_rule_for_launch(
                plan.profile_dir.as_deref(),
                plan.stale_profile.is_some(),
                &rule
            ),
            ProfileRule::Pinned("Profile 1".into())
        );

        // No pin at all: no switch, and the raise keeps the binding's own rule.
        let unpinned = KeyBinding {
            app: Some(GHOST_EXE.into()),
            ..Default::default()
        };
        let plan = app_launch_plan(&unpinned);
        assert!(plan.params.is_none());
        assert!(plan.profile_dir.is_none());
        assert!(plan.stale_profile.is_none());
        let mine = ProfileRule::Unpinned(vec!["Profile 1".into()]);
        assert_eq!(
            raise_rule_for_launch(plan.profile_dir.as_deref(), false, &mine),
            mine
        );
    }

    /// The other half of the same coin, stated so a future edit cannot quietly
    /// re-import the app leg's rule here: a URL binding takes NO claims
    /// argument, and its rule is a function of the route alone. The app leg is
    /// where the owner's unpinned rule lives, because that leg has no site
    /// keyword to narrow candidates with.
    #[test]
    fn the_url_leg_and_the_app_leg_disagree_on_purpose() {
        let claims = vec![("brave".to_string(), "Profile 1".to_string())];

        // App leg: an unpinned Brave binding declines Space+N's window.
        let app_rule = rule_for_binding(&brave(None), BRAVE_EXE, &claims, &nothing_exists);
        assert!(!evidence_satisfies(
            &WindowProfile::Named("Profile 1".into()),
            &app_rule
        ));

        // URL leg on the same machine: a plain youtube.com key still toggles
        // the window showing youtube, whichever profile it lives in.
        let url_rule = url_rule(&url_binding(None, None), &everything_exists);
        assert!(evidence_satisfies(
            &WindowProfile::Named("Profile 1".into()),
            &url_rule
        ));
    }

    // -------------------------------------------------------------- TASK 2 + 3
    // What a window's evidence buys it.

    #[test]
    fn any_accepts_every_window_including_an_unreadable_one() {
        for ev in [
            WindowProfile::Named("Profile 1".into()),
            WindowProfile::NoProfileNamed,
            WindowProfile::Unknown,
        ] {
            assert!(evidence_satisfies(&ev, &ProfileRule::Any));
        }
    }

    #[test]
    fn a_pinned_binding_matches_its_own_profile_in_either_spelling() {
        let rule = ProfileRule::Pinned("Profile 1".into());
        assert!(evidence_satisfies(
            &WindowProfile::Named("Profile 1".into()),
            &rule
        ));
        // The AUMID spelling of the same profile. If this ever fails the
        // feature is dead and nothing else will say so.
        assert!(evidence_satisfies(
            &WindowProfile::Named("Profile1".into()),
            &rule
        ));
    }

    /// THE RULE THAT MATTERS MOST. Missing, unreadable, or a different
    /// profile — all three mean "not proven mine", and all three must fall
    /// through to launch. When we cannot prove a window belongs to this
    /// binding, launching is safe and minimising is not; that is the class of
    /// bug NATIVE_SAFETY.md exists to prevent.
    #[test]
    fn a_pinned_binding_refuses_everything_it_cannot_prove() {
        let rule = ProfileRule::Pinned("Profile 1".into());
        assert!(!evidence_satisfies(
            &WindowProfile::Named("Profile 2".into()),
            &rule
        ));
        assert!(!evidence_satisfies(&WindowProfile::NoProfileNamed, &rule));
        assert!(!evidence_satisfies(&WindowProfile::Unknown, &rule));
    }

    /// The owner's rule for unpinned bindings, in one test: B takes whatever
    /// Brave window is not N's, rather than stealing it.
    #[test]
    fn an_unpinned_binding_takes_what_nobody_else_has_claimed() {
        let rule = ProfileRule::Unpinned(vec!["Profile 1".into()]);
        assert!(evidence_satisfies(
            &WindowProfile::Named("Profile 4".into()),
            &rule
        ));
        assert!(!evidence_satisfies(
            &WindowProfile::Named("Profile 1".into()),
            &rule
        ));
        // Claimed, spelled the AUMID way. The normalisation has to hold on this
        // side too or the claim quietly stops protecting anything.
        assert!(!evidence_satisfies(
            &WindowProfile::Named("Profile1".into()),
            &rule
        ));
    }

    /// A window whose relaunch command names NO profile cannot be one of the
    /// explicitly named `Profile N` claims, so an unpinned binding may still
    /// toggle it — which is what keeps Space+B working normally while Space+N
    /// owns Profile 1. If somebody has actually claimed `Default`, it declines.
    #[test]
    fn a_window_naming_no_profile_is_free_unless_default_itself_is_claimed() {
        assert!(evidence_satisfies(
            &WindowProfile::NoProfileNamed,
            &ProfileRule::Unpinned(vec!["Profile 1".into()])
        ));
        assert!(!evidence_satisfies(
            &WindowProfile::NoProfileNamed,
            &ProfileRule::Unpinned(vec!["Default".into()])
        ));
    }

    /// We are only ever in `Unpinned` because the user HAS pinned a profile of
    /// this browser somewhere. An unidentifiable window in that world is
    /// exactly the ambiguity the fail-safe is for: this key focuses by
    /// relaunching instead of toggling, which is a papercut. The alternative is
    /// the reported bug, back again.
    #[test]
    fn an_unpinned_binding_still_refuses_an_unidentifiable_window() {
        assert!(!evidence_satisfies(
            &WindowProfile::Unknown,
            &ProfileRule::Unpinned(vec!["Profile 1".into()])
        ));
    }

    /// THE INCIDENT, end to end at the pure level. Space+B is unpinned Brave,
    /// Space+N is Brave "Profile 1", and HWND 0x40966 is a Profile 1 window.
    /// Before this change both keys matched it and shared its cache entry.
    #[test]
    fn the_reported_incident_no_longer_reproduces() {
        let b = brave(None);
        let n = brave(Some("Profile 1"));
        let claims = crate::browser_profiles::profile_claims([&b, &n]);

        let b_rule = rule_for_binding(&b, BRAVE_EXE, &claims, &nothing_exists);
        let n_rule = rule_for_binding(&n, BRAVE_EXE, &claims, &nothing_exists);

        // Half 1: they are no longer the same cache entry.
        assert_ne!(cache_key("brave", &b_rule), cache_key("brave", &n_rule));

        // Half 2: they no longer both accept the same window.
        let profile_1_window = WindowProfile::Named("Profile 1".into());
        assert!(evidence_satisfies(&profile_1_window, &n_rule));
        assert!(
            !evidence_satisfies(&profile_1_window, &b_rule),
            "Space+B must not take Space+N's window — it falls through to launch, \
             and Brave itself puts the right window in front"
        );

        // And B is not left with nothing: an ordinary Brave window is still its.
        let ordinary_window = WindowProfile::Named("Profile 4".into());
        assert!(evidence_satisfies(&ordinary_window, &b_rule));
        assert!(!evidence_satisfies(&ordinary_window, &n_rule));
    }
}

/// PROBLEM 225 — the post-launch raise's two judgements, exercised at the pure
/// level.
///
/// These exist because the bug they replace was invisible to review. The old
/// code read like a reasonable rule ("stand down if the user typed"), compiled,
/// and was wrong 57 times in six days — and there was no test that could have
/// said so, because the decision was three lines welded to `GetForegroundWindow`
/// and a static in another module. Every case below is a line from the owner's
/// 2026-08-25..31 log.
#[cfg(test)]
mod raise_decision_tests {
    use super::*;

    fn stems(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    // --- typing_stand_down ------------------------------------------------

    /// THE REPORTED BUG. Brave, 2026-08-31 08:33 — launch at .971, stand-down at
    /// 55.805, i.e. 834 ms later, on the Space-UP of the combo that fired it.
    /// Inside the grace the answer is no, whatever the tick says.
    #[test]
    fn a_key_inside_the_grace_never_stands_down() {
        assert!(!typing_stand_down(834, 1_000, 1_500));
        assert!(!typing_stand_down(0, 1_000, 9_999));
        // The boundary belongs to the grace on the closed side: at exactly
        // RAISE_GRACE_MS we are out of it.
        assert!(!typing_stand_down(RAISE_GRACE_MS - 1, 1_000, 1_500));
        assert!(typing_stand_down(RAISE_GRACE_MS, 1_000, 1_500));
    }

    /// The case the stand-down actually exists for: the window was so late that
    /// the owner started typing into something else.
    #[test]
    fn typing_after_the_grace_stands_down() {
        assert!(typing_stand_down(2_300, 1_000, 1_010));
    }

    /// A quiet keyboard must never stand the watcher down, at any age. The tick
    /// not moving is the only thing that means "no key went down".
    #[test]
    fn an_unmoved_tick_never_stands_down() {
        assert!(!typing_stand_down(7_999, 1_000, 1_000));
        assert!(!typing_stand_down(7_999, 0, 0));
    }

    // --- foreign_foreground_verdict ---------------------------------------

    /// An unchanged foreground is not a signal, and neither is a changed one
    /// inside the grace — that is the app's own splash screen, which used to
    /// stand the watcher down against the very launch that created it.
    #[test]
    fn no_verdict_before_the_grace_or_without_a_change() {
        assert_eq!(
            foreign_foreground_verdict(5_000, false, None, 0, &stems(&["brave"])),
            ForeignVerdict::KeepPolling
        );
        assert_eq!(
            foreign_foreground_verdict(
                900,
                true,
                Some((4242, "chrome")),
                0,
                &stems(&["brave"])
            ),
            ForeignVerdict::KeepPolling
        );
    }

    /// POSITIVE IDENTITY beats the stem matcher. This is the `whatsapp://` case
    /// from PROBLEM 216: the binding says "whatsapp", the process the shell
    /// started is `WhatsApp.Root`, and only the PID knows they are the same
    /// launch.
    #[test]
    fn the_pid_we_launched_is_never_foreign() {
        assert_eq!(
            foreign_foreground_verdict(
                3_000,
                true,
                Some((7788, "whatsapp.root")),
                7788,
                &stems(&["whatsapp"])
            ),
            ForeignVerdict::KeepPolling
        );
    }

    /// pid 0 means "the shell created no process" (a folder, a DDE or Store
    /// activation). It must never match a real PID — a foreground window whose
    /// process id happened to be 0 would otherwise read as ours.
    #[test]
    fn an_unknown_launch_pid_matches_nothing() {
        assert_eq!(
            foreign_foreground_verdict(3_000, true, Some((0, "explorer")), 0, &stems(&["downloads"])),
            ForeignVerdict::StandDown
        );
    }

    /// The stem list is the second identity, and it is case-insensitive because
    /// `launch_stem` lowercases while `QueryFullProcessImageNameW` does not
    /// promise to.
    #[test]
    fn a_stem_match_is_not_foreign_and_ignores_case() {
        assert_eq!(
            foreign_foreground_verdict(3_000, true, Some((99, "Brave")), 0, &stems(&["brave"])),
            ForeignVerdict::KeepPolling
        );
    }

    /// A genuinely different application, late. This is the one true positive
    /// the whole branch exists for.
    #[test]
    fn a_third_application_after_the_grace_stands_down() {
        assert_eq!(
            foreign_foreground_verdict(
                4_500,
                true,
                Some((31337, "winword")),
                7788,
                &stems(&["brave", "brave-browser"])
            ),
            ForeignVerdict::StandDown
        );
    }

    /// THE FAIL-SAFE, and the reason `Unidentified` is a third variant rather
    /// than a `bool`. We could not name the process, so we know nothing about
    /// the user — and saying nothing (keep polling, raise nothing at the
    /// deadline) is this file's documented safe failure. Collapsing this into
    /// `StandDown` would re-create the bug for every window we lack rights to
    /// query.
    #[test]
    fn an_unidentifiable_foreground_is_not_a_stand_down() {
        assert_eq!(
            foreign_foreground_verdict(4_500, true, None, 7788, &stems(&["brave"])),
            ForeignVerdict::Unidentified
        );
    }
}

/// A LIVE, READ-ONLY look at the windows on this machine right now.
///
/// Ignored by default: its result depends on which browsers happen to be open,
/// so it is a diagnostic, not a pass/fail gate. It touches nothing — no
/// `ShowWindow`, no `SetForegroundWindow`, no config — it only enumerates and
/// reads window properties, so it is safe to run against the owner's live
/// session while he is using the machine.
///
/// WHY IT EXISTS. Everything the profile matcher knows was measured through
/// PowerShell COM interop. That proves the PROPERTIES are there; it does not
/// prove this crate reads them correctly, and "the property store returned
/// nothing" is indistinguishable from "no window matched" in every log line we
/// have. This closes that gap without an install: it prints what each browser
/// window reports, then puts the PRODUCTION matcher against those live windows
/// and asserts it agrees — including the space-stripped AUMID spelling, which
/// is the one mistake that would silently match nothing forever.
///
/// Run: `cargo test --lib -- --ignored --nocapture live_window_profiles`
#[cfg(all(test, windows))]
mod live_profile_probe {
    use super::*;
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowTextLengthW, GetWindowTextW, IsWindowVisible,
    };

    /// exe stem, window title, the RAW RelaunchCommand, the RAW AUMID, and
    /// what the matcher made of them. The raw pair is printed because a future
    /// reader needs to see the SHAPE, not only our interpretation of it — that
    /// is how the Default-profile question got answered.
    struct Collect(Vec<(String, String, String, String, WindowProfile)>);

    unsafe extern "system" fn cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
        use windows::Win32::System::Threading::{
            OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
            PROCESS_QUERY_LIMITED_INFORMATION,
        };
        use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;

        let out = &mut *(lparam.0 as *mut Collect);
        if !IsWindowVisible(hwnd).as_bool() || GetWindowTextLengthW(hwnd) == 0 {
            return BOOL(1);
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return BOOL(1);
        }
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return BOOL(1);
        };
        let mut buf = [0u16; 260];
        let mut size = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(
            h,
            PROCESS_NAME_FORMAT(0),
            windows::core::PWSTR(buf.as_mut_ptr()),
            &mut size,
        );
        let _ = windows::Win32::Foundation::CloseHandle(h);
        if ok.is_err() {
            return BOOL(1);
        }
        let path = String::from_utf16_lossy(&buf[..size as usize]);
        let stem = std::path::Path::new(&path)
            .file_stem()
            .map(|s| s.to_string_lossy().to_lowercase())
            .unwrap_or_default();

        let evidence = window_profile_evidence(hwnd);
        if !matches!(evidence, WindowProfile::Unknown) {
            use windows::Win32::UI::Shell::PropertiesSystem::PROPERTYKEY;
            let raw_relaunch = window_string_property(
                hwnd,
                &PROPERTYKEY { fmtid: PKEY_APPUSERMODEL_FMTID, pid: 2 },
            )
            .unwrap_or_default();
            let raw_aumid = window_string_property(
                hwnd,
                &PROPERTYKEY { fmtid: PKEY_APPUSERMODEL_FMTID, pid: 5 },
            )
            .unwrap_or_default();
            let mut tbuf = [0u16; 256];
            let n = GetWindowTextW(hwnd, &mut tbuf);
            let title = String::from_utf16_lossy(&tbuf[..n.max(0) as usize]);
            out.0.push((stem, title, raw_relaunch, raw_aumid, evidence));
        }
        BOOL(1)
    }

    #[test]
    #[ignore]
    fn live_window_profiles() {
        let mut out = Collect(Vec::new());
        unsafe {
            let _com = ComGuard::new();
            let _ = EnumWindows(Some(cb), LPARAM(&mut out as *mut Collect as isize));
        }

        println!(
            "\nwindows carrying Chromium profile evidence: {}",
            out.0.len()
        );
        for (stem, title, raw_relaunch, raw_aumid, evidence) in &out.0 {
            let short: String = title.chars().take(60).collect();
            println!("\n  {stem}  ->  {evidence:?}");
            println!("     title    {short}");
            println!("     AUMID    {raw_aumid}");
            println!("     Relaunch {raw_relaunch}");
        }
        assert!(
            !out.0.is_empty(),
            "no window reported a profile — open a Chromium browser and re-run"
        );

        // Now the production matcher, against those same live windows.
        for (stem, _, _, _, evidence) in &out.0 {
            let WindowProfile::Named(dir) = evidence else {
                continue;
            };
            assert!(
                find_window_by_exe_stem(stem, &ProfileRule::Pinned(dir.clone())).is_some(),
                "pinning {dir:?} found no {stem} window, but one is open right now"
            );

            // The AUMID spelling of the SAME profile ("Profile1" for
            // "Profile 1"). If normalisation is wrong this finds nothing, and
            // nothing else in the app would ever say so.
            let squashed = dir.replace(' ', "");
            assert!(
                find_window_by_exe_stem(stem, &ProfileRule::Pinned(squashed.clone())).is_some(),
                "the space-stripped spelling {squashed:?} matched nothing — the AUMID trap is live"
            );

            // And the fail-safe: a profile nobody has must match nothing at all.
            assert!(
                find_window_by_exe_stem(
                    stem,
                    &ProfileRule::Pinned("Profile 4242".into())
                )
                .is_none(),
                "a profile that does not exist matched a {stem} window — the fail-safe is broken"
            );

            // The owner's rule, live: an UNPINNED binding must decline exactly
            // this window and nothing forces it to.
            let claimed = ProfileRule::Unpinned(vec![dir.clone()]);
            assert!(
                !evidence_satisfies(evidence, &claimed),
                "an unpinned binding took a window another binding had claimed"
            );
        }
    }
}

/* ===========================================================================
   TESTS — PROBLEM 216, the launch spine.
   ===========================================================================
   The important one is `every_binding_shape_can_be_matched_or_says_why`. It
   does NOT test WhatsApp; it tests the INVARIANT, over every shape at once, so
   the day someone adds a fifth branch the test is what fails. The `match` in
   `shape_coverage_is_exhaustive_by_construction` has no wildcard arm, so a new
   `TargetShape` variant also fails to COMPILE here until it has been given a
   row.
   =========================================================================== */
#[cfg(test)]
mod launch_spine_tests {
    use super::*;

    /// Every shape of binding string a user can end up with, with a note on
    /// where it comes from. This list is the audit table from the docs entry,
    /// made executable.
    fn every_binding_shape() -> Vec<(&'static str, &'static str)> {
        vec![
            ("shell:AppsFolder\\5319275A.WhatsAppDesktop_cv1g1gvanyjgm!App",
             "Store app picked from the app grid"),
            ("C:\\Users\\x\\AppData\\Local\\Discord\\app-1.0.9255\\Discord.exe",
             "absolute exe path — the dashboard's normal shape"),
            ("C:\\ProgramData\\Microsoft\\Windows\\Start Menu\\Programs\\NVIDIA App.lnk",
             "absolute .lnk — the picker keeps shortcuts whose arguments matter"),
            ("whatsapp.exe", "bare name the protocol table knows (Store-packaged handler)"),
            ("discord.exe", "bare name the protocol table knows (classic exe handler)"),
            ("notepad.exe", "bare name resolved through the known-paths table"),
            ("Battle.net.exe", "bare name that only the Start Menu can resolve"),
        ]
    }

    /// THE INVARIANT: a binding may either be matched, or say out loud why it
    /// cannot be. There is no third outcome, and "the author forgot" is not one.
    #[test]
    fn every_binding_shape_can_be_matched_or_says_why() {
        for (target, whence) in every_binding_shape() {
            let plan = resolve_launch_plan(target);
            match &plan.identity {
                Identities::Try(ids) => assert!(
                    !ids.is_empty(),
                    "{target:?} ({whence}) produced an EMPTY identity list — that is the \
                     bug PROBLEM 216 fixed, and `LaunchPlan::matchable` is supposed to \
                     make it unspellable"
                ),
                Identities::Unmatchable(why) => assert!(
                    !why.trim().is_empty(),
                    "{target:?} ({whence}) is unmatchable with no reason given — an \
                     unmatchable binding must NAME its reason, because that string is \
                     the only thing a future log reader will have"
                ),
            }
        }
    }

    /// No wildcard arm: adding a `TargetShape` variant fails to compile here
    /// until somebody has decided what it means. That is the structural half of
    /// the guarantee; the test above is the behavioural half.
    #[test]
    fn shape_coverage_is_exhaustive_by_construction() {
        for (target, _) in every_binding_shape() {
            let named = match classify_target(target) {
                TargetShape::ShellVerb => "shell verb",
                TargetShape::AbsolutePath => "absolute path",
                TargetShape::ProtocolUri(_) => "protocol uri",
                TargetShape::BareName => "bare name",
            };
            assert!(!named.is_empty());
        }
    }

    #[test]
    fn classification_matches_the_ladder_it_replaced() {
        assert_eq!(classify_target("shell:AppsFolder\\Foo!App"), TargetShape::ShellVerb);
        assert_eq!(classify_target("C:\\Apps\\Brave\\brave.exe"), TargetShape::AbsolutePath);
        assert_eq!(
            classify_target("whatsapp.exe"),
            TargetShape::ProtocolUri("whatsapp://".into())
        );
        assert_eq!(classify_target("notepad.exe"), TargetShape::BareName);
        // Order matters and always did: an ABSOLUTE path to discord.exe is a
        // path, not a protocol activation. Reversing these two would send every
        // dashboard-set Discord binding through the URI route — the one route
        // that could not match.
        assert_eq!(
            classify_target("C:\\Users\\x\\AppData\\Local\\Discord\\app-1.0.9255\\Discord.exe"),
            TargetShape::AbsolutePath
        );
    }

    #[test]
    fn a_store_binding_is_matched_by_aumid_and_its_raise_is_skipped() {
        let t = "shell:AppsFolder\\5319275A.WhatsAppDesktop_cv1g1gvanyjgm!App";
        let plan = resolve_launch_plan(t);
        assert_eq!(
            plan.identity,
            Identities::Try(vec![MatchIdentity::Aumid(t.to_string())])
        );
        assert!(matches!(plan.raise, RaiseIdentity::Skip(_)));
    }

    #[test]
    fn a_plain_absolute_exe_is_unchanged_from_before_the_spine() {
        let t = "C:\\Users\\x\\AppData\\Local\\Discord\\app-1.0.9255\\Discord.exe";
        let plan = resolve_launch_plan(t);
        assert_eq!(
            plan.identity,
            Identities::Try(vec![MatchIdentity::ExeStem(t.to_string())]),
            "the branch that WORKS today must keep exactly the identity it had — the \
             whole binding string, so the Event/Target log line still names the path"
        );
        assert_eq!(plan.raise, RaiseIdentity::ExeStems(vec!["discord".into()]));
    }

    #[test]
    fn a_shortcut_also_gets_the_identity_of_what_it_points_at() {
        // The shortcut's NAME is not the target's process name. This is the
        // second instance of PROBLEM 216's class, and the one that reaches
        // users who never bind a protocol URI at all.
        let t = "C:\\ProgramData\\Microsoft\\Windows\\Start Menu\\Programs\\NVIDIA App.lnk";
        let plan = resolve_launch_plan(t);
        assert_eq!(
            plan.identity,
            Identities::Try(vec![
                MatchIdentity::ExeStem(t.to_string()),
                MatchIdentity::ResolvedExeStem(t.to_string()),
            ])
        );
        assert_eq!(plan.raise, RaiseIdentity::Resolved(t.to_string()));
    }

    #[test]
    fn a_bare_name_is_matched_cheaply_first_and_by_its_resolution_second() {
        let plan = resolve_launch_plan("Battle.net.exe");
        assert_eq!(
            plan.identity,
            Identities::Try(vec![
                MatchIdentity::ExeStem("Battle.net.exe".into()),
                MatchIdentity::ResolvedExeStem("Battle.net.exe".into()),
            ]),
            "the cheap stem must stay FIRST — it is what every working binding \
             already uses, and resolution can walk two Start Menu trees"
        );
    }

    #[test]
    fn a_protocol_binding_keeps_its_own_stem_first_and_gains_the_handlers_identity() {
        // Machine-independent on purpose: whatever this machine says handles
        // whatsapp://, the binding's own stem is still identity #0 (so nothing
        // regresses) and the list is never empty (so the branch that used to
        // have NO match leg can never have one again).
        let plan = resolve_launch_plan("whatsapp.exe");
        match plan.identity {
            Identities::Try(ids) => {
                assert_eq!(ids[0], MatchIdentity::ExeStem("whatsapp.exe".into()));
            }
            Identities::Unmatchable(why) => panic!("a protocol binding went unmatchable: {why}"),
        }
    }

    #[test]
    fn a_binding_that_names_no_file_is_unmatchable_with_a_reason() {
        // The fall-through the whole design rests on: no identity, an explicit
        // named reason, and today's unconditional launch underneath it.
        let plan = resolve_launch_plan("");
        match plan.identity {
            Identities::Unmatchable(why) => assert!(why.contains("names no file")),
            other => panic!("expected an explicit unmatchable, got {other:?}"),
        }
    }

    #[test]
    fn an_identity_list_cannot_be_built_empty_and_never_repeats_itself() {
        let plan = LaunchPlan::matchable(
            TargetShape::BareName,
            MatchIdentity::ExeStem("discord".into()),
            vec![
                MatchIdentity::ExeStem("discord".into()),
                MatchIdentity::ExeStem("discord".into()),
            ],
            RaiseIdentity::ExeStems(vec!["discord".into()]),
        );
        assert_eq!(
            plan.identity,
            Identities::Try(vec![MatchIdentity::ExeStem("discord".into())]),
            "a duplicate identity means a duplicate EnumWindows on the Space-hold path"
        );
    }

    // -----------------------------------------------------------------------
    // The scheme-to-handler parsers. Both shapes below are REAL VALUES read off
    // this machine on 2026-08-29 — see `live_protocol_probe`.
    // -----------------------------------------------------------------------

    #[test]
    fn the_quoted_shell_open_command_shape_parses() {
        assert_eq!(
            exe_from_shell_open_command(
                "\"C:\\Users\\beamu\\AppData\\Local\\Discord\\app-1.0.9255\\Discord.exe\" --url -- \"%1\""
            ),
            Some("C:\\Users\\beamu\\AppData\\Local\\Discord\\app-1.0.9255\\Discord.exe".into())
        );
        assert_eq!(
            exe_from_shell_open_command(
                "\"C:\\Users\\beamu\\AppData\\Roaming\\Spotify\\spotify.exe\" --protocol-uri=\"%1\""
            ),
            Some("C:\\Users\\beamu\\AppData\\Roaming\\Spotify\\spotify.exe".into())
        );
    }

    #[test]
    fn the_unquoted_shell_open_command_shape_parses_too() {
        // A parser that only understood the quoted form would return nothing
        // here, silently, forever — the same failure PROBLEM 207 measured when
        // Chrome quoted its profile folder and Edge did not.
        assert_eq!(
            exe_from_shell_open_command("C:\\Windows\\notepad.exe %1"),
            Some("C:\\Windows\\notepad.exe".into())
        );
        // Unquoted AND containing spaces: cut after the .exe, not at the first
        // space, or the path is truncated to "C:\\Program".
        assert_eq!(
            exe_from_shell_open_command("C:\\Program Files\\Foo\\foo.exe -- \"%1\""),
            Some("C:\\Program Files\\Foo\\foo.exe".into())
        );
    }

    #[test]
    fn an_unusable_shell_open_command_yields_nothing_rather_than_rubbish() {
        assert_eq!(exe_from_shell_open_command(""), None);
        assert_eq!(exe_from_shell_open_command("   "), None);
        assert_eq!(exe_from_shell_open_command("\"\" %1"), None);
    }

    #[test]
    fn reg_expand_sz_command_lines_are_expanded() {
        std::env::set_var("ST_TEST_ROOT", "C:\\Windows");
        assert_eq!(
            expand_env_vars("%ST_TEST_ROOT%\\notepad.exe %1"),
            "C:\\Windows\\notepad.exe %1"
        );
        // An unknown variable is kept VERBATIM rather than deleted, so a log
        // reader sees what failed to expand instead of a mangled path.
        assert_eq!(expand_env_vars("%NoSuchVar_1234%\\x.exe"), "%NoSuchVar_1234%\\x.exe");
        assert_eq!(expand_env_vars("no vars here"), "no vars here");
        assert_eq!(expand_env_vars("trailing %"), "trailing %");
    }

    #[test]
    fn scheme_names_come_off_uris_and_junk_is_refused() {
        assert_eq!(scheme_of_uri("whatsapp://"), Some("whatsapp"));
        assert_eq!(scheme_of_uri("discord://"), Some("discord"));
        assert_eq!(scheme_of_uri("mailto:someone"), Some("mailto"));
        assert_eq!(scheme_of_uri("C:\\x\\y.exe"), None);
        assert_eq!(scheme_of_uri("://nothing"), None);
    }
}

/// READ-ONLY PROBE — what does this machine actually say about each protocol?
///
/// PROBLEM 216 was diagnosed from a live log and fixed from THIS data, and the
/// data is the part a future reader cannot re-derive by reading code. It prints
/// the registry command, the AssocQueryString AUMID, and the identities the
/// production code builds from them, for every scheme in `protocol_uri`'s
/// table. Touches nothing.
///
/// Run: `cargo test --lib -- --ignored --nocapture live_protocol_probe`
#[cfg(all(test, windows))]
mod live_protocol_probe {
    use super::*;

    #[test]
    #[ignore]
    fn what_each_protocol_resolves_to_on_this_machine() {
        println!("\n=== protocol -> handler, as the cascade sees it ===");
        for exe in ["discord.exe", "spotify.exe", "whatsapp.exe", "steam.exe"] {
            let Some(uri) = protocol_uri(exe) else { continue };
            let scheme = scheme_of_uri(&uri).unwrap_or("").to_string();
            println!("\n{exe}  ->  {uri}");
            println!("  HKCR\\{scheme}\\shell\\open\\command exe : {:?}", scheme_handler_exe(&scheme));
            println!("  AssocQueryString ASSOCSTR_APPID     : {:?}", scheme_handler_aumid(&scheme));
            println!("  identities the cascade will try     : {:?}", scheme_identities(&scheme));
            println!("  full plan                           : {:?}", resolve_launch_plan(exe));
        }
        println!("\n=== and what a bare name resolves to ===");
        for name in ["notepad.exe", "Battle.net.exe", "brave.exe"] {
            println!("{name:>16}  resolve_path -> {:?}", resolve_path(name));
            println!("{:>16}  match stem   -> {:?}", "", resolved_target_stem(name));
        }
    }
}

/// PROBLEM 227 — the cache-hit branch, tested where it can be tested.
///
/// Every case below is a window a FRESH ENUMERATION would have refused, arriving
/// through the cache instead. The bug this closes is that the cache path agreed
/// with the enumeration only for explorer bindings and pinned browser profiles,
/// and disagreed with it for every ordinary binding on the machine.
#[cfg(test)]
mod cached_hwnd_tests {
    use super::*;
    use std::cell::Cell;

    /// The ordinary, healthy cache hit: the handle still points at the app the
    /// binding names. Nothing here may change — this is 99.9% of presses.
    #[test]
    fn a_cache_hit_on_the_right_window_still_acts() {
        assert_eq!(
            cached_window_verdict(true, Some("notepad"), "Notepad", true, "notepad", || true),
            Ok(())
        );
    }

    /// **THE RECYCLED HANDLE.** Space+N is bound to `notepad`; the cached HWND
    /// was Notepad's, Notepad was closed, and Windows handed that same handle
    /// value to a shell window (the session handle table is shared — NATIVE_SAFETY
    /// rule 3). `IsWindow` says true, and before this fix that was the ONLY
    /// surviving check on this path: the class filter was gated on the BINDING
    /// being explorer (it is not, it is notepad) and the profile filter was gated
    /// on the rule not being `Any` (it is `Any`). So both guards short-circuited
    /// and the branch went straight to `SW_MINIMIZE` / `SW_RESTORE` +
    /// `force_foreground` — on the taskbar. That is the 2026-08-10 touchpad
    /// incident, reached through a path that never looks at a class.
    #[test]
    fn a_recycled_handle_now_pointing_at_the_shell_is_never_acted_on() {
        assert_eq!(
            cached_window_verdict(true, Some("explorer"), "Shell_TrayWnd", false, "notepad", || true),
            Err(CacheEvict::WrongExecutable),
            "the binding is notepad and the handle now belongs to explorer.exe — \
             acting on it is how the shell gets minimised"
        );
        // Same shape, an ordinary third-party app rather than the shell: still
        // not this binding's window, still must not be touched.
        assert_eq!(
            cached_window_verdict(true, Some("brave"), "Chrome_WidgetWin_1", true, "notepad", || true),
            Err(CacheEvict::WrongExecutable)
        );
    }

    /// The class rule must be keyed on what the handle points at NOW, not on
    /// what the user bound. Keyed on the binding it asks "did the user bind
    /// explorer?", which is not a safety check at all.
    #[test]
    fn the_explorer_class_rule_reads_the_live_window() {
        // A genuine File Explorer file window: the one explorer class we may
        // ever touch.
        assert_eq!(
            cached_window_verdict(true, Some("explorer"), "CabinetWClass", true, "explorer", || true),
            Ok(())
        );
        // Everything else explorer.exe owns is shell infrastructure.
        for cls in ["Shell_TrayWnd", "WorkerW", "Progman", "XamlExplorerHostIslandWindow",
                    "ThumbnailDeviceHelperWnd"] {
            assert_eq!(
                cached_window_verdict(true, Some("explorer"), cls, true, "explorer", || true),
                Err(CacheEvict::ShellWindow),
                "{cls} is the shell, not a File Explorer window"
            );
        }
    }

    /// `window_process_identity` returns `None` for pid 0, a process we may not
    /// open, or a failed image-name query. `enum_callback` never SELECTS such a
    /// window, so the cache may not act on one either. "Unknown" is not "ours".
    #[test]
    fn a_handle_whose_process_cannot_be_named_is_evicted() {
        assert_eq!(
            cached_window_verdict(true, None, "", false, "notepad", || true),
            Err(CacheEvict::UnprovableIdentity)
        );
    }

    /// The enum path's empty-title rule, which exists because helper, IME, tray
    /// and splash-leftover windows are infrastructure rather than the app the
    /// user meant (NATIVE_SAFETY, same incident: a cascade once "restored"
    /// `ThumbnailDeviceHelperWnd`).
    #[test]
    fn an_untitled_window_of_the_right_executable_is_evicted() {
        assert_eq!(
            cached_window_verdict(true, Some("discord"), "Chrome_WidgetWin_1", false, "discord", || true),
            Err(CacheEvict::NoTitle)
        );
    }

    #[test]
    fn a_dead_handle_is_evicted_and_reads_nothing() {
        let asked = Cell::new(false);
        assert_eq!(
            cached_window_verdict(false, None, "", false, "notepad", || {
                asked.set(true);
                true
            }),
            Err(CacheEvict::Dead)
        );
        assert!(!asked.get(), "a dead handle must not cost a property read");
    }

    /// The 2026-08-26 wrong-minimise (PROBLEM 207/220) must stay fixed: a window
    /// that can no longer prove it belongs to this binding's profile is evicted.
    #[test]
    fn a_window_that_cannot_prove_its_profile_is_still_evicted() {
        assert_eq!(
            cached_window_verdict(true, Some("brave"), "Chrome_WidgetWin_1", true, "brave", || false),
            Err(CacheEvict::WrongProfile)
        );
    }

    /// SAFETY FILTERS FIRST, ACCEPTANCE LAST — and the expensive question is
    /// asked last of all. A window that already failed the identity or class
    /// test must not cost a Chromium property-store read (which is COM, on the
    /// Space-hold dispatch path).
    #[test]
    fn the_profile_read_is_never_paid_for_by_a_window_that_already_failed() {
        for (live, class, titled, binding) in [
            (Some("explorer"), "Shell_TrayWnd", false, "notepad"),
            (Some("explorer"), "WorkerW", true, "explorer"),
            (Some("discord"), "Chrome_WidgetWin_1", false, "discord"),
            (None, "", false, "brave"),
        ] {
            let asked = Cell::new(false);
            let verdict = cached_window_verdict(true, live, class, titled, binding, || {
                asked.set(true);
                true
            });
            assert!(verdict.is_err());
            assert!(
                !asked.get(),
                "{binding}/{class}: the profile read must come after every cheaper filter"
            );
        }
    }
}
