/// engine/actions/pip.rs — Picture-in-Picture 4-corner cycle.
/// Mirrors V11 TogglePiP(), rebuilt 2026-08-24 (PROBLEM 167).
///
/// WHAT CHANGED AND WHY — read before "restoring" any of it.
///
/// The owner's report: *"pip isn't working properly… it behaves oddly and
/// doesn't go to the 4 corners properly either, loses its title bar and won't
/// come back."* Then, live: *"I tried PiP, it didn't work for the Claude
/// desktop app, then space hud sound appeared but showed nothing, it appeared
/// behind Claude."* That last sentence is the whole bug in one line — see §3.
///
/// 1. NO MORE STYLE SURGERY. The old code stripped WS_CAPTION|WS_THICKFRAME
///    to make a clean borderless tile. On classic windows that looked good. On
///    Electron/Chromium windows — Claude, Discord, VS Code, Spotify, i.e. most
///    of what this owner actually uses — those bits are not what draws the
///    frame, so stripping them changed nothing visible while still arming the
///    failure below. Owner's call, 2026-08-24: "stop stripping entirely —
///    corner-snap only." PiP is now move + resize + stay-on-top. Nothing about
///    another program's window structure is modified, so there is nothing that
///    can fail to be put back.
///
/// 2. THE ORPHAN. `restore_window` was reachable ONLY on the 5th tap, and this
///    cache is memory-only. Restart Spaceadom — or lose the entry any other
///    way — and that window stayed borderless, topmost and half-size FOREVER,
///    with no route back and no visible sign of what had happened to it. There
///    was no restore-on-exit handler and no `IsWindow` validation, so a closed
///    window's entry also sat in the map until the process died, ready for
///    Windows to hand its recycled HWND to something innocent.
///    Now: entries are validated every pass, `restore_all()` runs on exit, and
///    `rescue_orphan()` repairs a window the OLD build stripped.
///
/// 3. THE TOPMOST POLLUTION — why the Guide HUD vanished behind Claude.
///    PiP marks its window HWND_TOPMOST and (before this) only cleared it on
///    the 5th tap. A window that could not be cycled out of therefore stayed
///    topmost for the rest of its life. The overlay's own "re-assert topmost
///    on every show" could not climb back over it, because that call was a
///    no-op (PROBLEM 168). So one failed PiP permanently hid the HUD behind
///    an ordinary-looking app. Both halves are fixed; this half is: topmost is
///    cleared on restore, on rescue, on exit — and now also when the window
///    itself is enlarged (§7).
///
/// 4. THE MONITOR IS THE CURSOR'S, DELIBERATELY. Corners are computed from
///    `MonitorFromPoint(GetCursorPos())` while the window comes from
///    `GetForegroundWindow()`, so with two displays PiP throws the window onto
///    the screen you are pointing at. That reads as a bug and is not one —
///    owner's explicit decision, 2026-08-24, when offered the alternative:
///    "the monitor the cursor is on — keep as is." Do not "fix" it.
///
/// 5. THE ANIMATION ACTUALLY RUNS NOW. The spring's exit test read
///    `(x - tx).abs() < 0.5 && vx.abs() < 0.5` — X ONLY. Top-Right → Bottom-
///    Right is a purely vertical hop, so x was already at target with vx = 0
///    and the loop broke on iteration 1: that tap snapped with no motion while
///    its neighbours glided. Both axes are tested now. Concurrent taps also
///    used to spawn overlapping threads that fought over one window, which is
///    the "behaves oddly" — a generation counter now retires the older one.
///
/// 6. ENTRY NO LONGER BLOCKS THE ENGINE ON THE TARGET APP (2026-08-26).
///    First entry used to call `ShowWindow(SW_RESTORE)` on a maximised window
///    BEFORE measuring it, because `GetWindowRect` on a maximised window
///    reports the maximised rect. That cross-thread `ShowWindow` does not
///    return until the TARGET's own WndProc has processed the full
///    un-maximise relayout. MAGNITUDE UNMEASURED ON THIS MACHINE — the
///    read-only investigation that established the mechanism saw log gaps of
///    only 6-41 ms and said so; the seconds-scale figures quoted around this
///    file come from the general research on wedged message pumps, NOT from a
///    stopwatch here. The mechanism is proven, the size is not, which is
///    exactly why the timing line below exists. On Electron/Chromium apps
///    (most of what this owner PiPs) that relayout is what made "entering PiP
///    the first time" laggy while corner-cycling — which never calls it —
///    felt fine. `GetWindowPlacement` dissolves the measure-first constraint:
///    `rcNormalPosition` holds the RESTORED bounds while the window is still
///    maximised. The un-maximise itself still has to happen (the corner tile
///    must not be a zoomed window), but it now runs on the animation thread,
///    where a slow foreign relayout can no longer stall the engine actor or
///    delay the toast. A timing line on the first-entry path (§ the entry
///    branch) reports where the milliseconds actually went, per call, so the
///    next lag report can be read off the log.
///
///    2026-08-26, review round 2: the un-maximise is no longer the FIRST
///    flight's private errand. Every flight checks `IsZoomed` and un-maximises
///    if it finds one. That is what stops a later corner tap from finishing
///    while the first tap is still blocked inside `ShowWindow` and then being
///    overwritten by the first tap's stale parting placement — the second
///    flight now queues behind the same serialized cross-thread call instead
///    of racing ahead of it. See `animate_to`.
///
/// 7. PiP RELEASES ITSELF WHEN ITS OWN WINDOW IS ENLARGED (2026-08-26).
///    Maximise or fullscreen a PiP'd window and it used to stay HWND_TOPMOST
///    at full size, floating over everything until cycled out or Spaceadom
///    quit. `release_enlarged()` — called from the EXISTING fullscreen
///    watcher tick, owner's decision, no new thread — drops topmost, forgets
///    the entry, and (the part that matters) writes the true pre-PiP bounds
///    back into the window's own `rcNormalPosition` first, because Windows
///    forgot them the moment PiP moved the window: without the write-back,
///    un-maximising after a release would land the user on a quarter-screen
///    corner tile whose real size nothing could ever recover. A DIFFERENT app
///    going fullscreen deliberately does NOT release PiP — floating over
///    other apps is the point of the feature.
///
///    THE HALF-RELEASE, and why the entry is sometimes KEPT (2026-08-26,
///    review round 2). `rcNormalPosition` can only be rewritten invisibly
///    while the window is MAXIMISED — that field is not displaying anything
///    then. A window taken to TRUE fullscreen (F11 in any Chromium app) is
///    NOT maximised: Chromium sends itself SC_RESTORE and then sizes the
///    window to the monitor, so `IsZoomed` is false and the window's normal
///    rect is live geometry. Writing the original bounds there would yank the
///    window out of fullscreen. The first cut of §7 dropped topmost, deleted
///    the entry and skipped the write anyway — which is bit-for-bit the
///    unrecoverable loss TASK 3b exists to forbid, just reached by F11
///    instead of the maximise button. So that case now does HALF a release:
///    topmost is dropped immediately (that is the bug the owner reported) and
///    the entry is KEPT as the last copy of the pre-PiP bounds. The 5th tap
///    and `restore_all` can still recover the window from it, and if the user
///    later maximises that window the watcher finishes the release properly.
///    This deviates from the letter of the owner's "remove it from the PiP
///    cache" only where obeying it would destroy data the amendment declares
///    must never be destroyed.
///
/// 8. A RELEASED ENTRY IS NOT AN ACTIVE PiP (TASK 1, 2026-08-26 — the owner's
///    overrule of §7's first cut, which the implementing agent flagged itself).
///    Keeping the entry is right. Keeping it UNCHANGED was not: the engine
///    reads the same map, and to it an entry is an entry, so the next tap on
///    the PiP key bumped `position_index` and flew the window to the NEXT
///    corner — while it sat behind everything, because only ENTRY asserts
///    HWND_TOPMOST and corner-cycling deliberately does not. A window hopping
///    between corners from behind every other app is not a state anyone asked
///    for.
///
///    So the retained entry is now marked `PipState::Released` and the tap
///    decision (`tap_for`) treats it as ABSENT for cycling purposes: the next
///    tap is a FRESH ENTRY — topmost re-asserted, corner 0, a new serial, a
///    fresh `entered_at`.
///
///    THE ONE THING RE-ENTRY MUST NOT DO IS MEASURE THE WINDOW. At that moment
///    the window is showing its FULLSCREEN rect (or, if the user has since
///    pressed F11 again, the corner tile PiP itself left behind — Windows
///    restores a Chromium window to its pre-fullscreen bounds, which were the
///    tile). Both are geometry PiP created or PiP is about to destroy;
///    capturing either as `original_*` would overwrite the last surviving copy
///    of the true pre-PiP bounds and recreate, on the 5th tap, exactly the
///    unrecoverable loss this whole design exists to prevent. Re-entry
///    therefore REUSES the preserved `original_*` and `was_maximized`
///    verbatim. The cost is that a window the user manually re-arranged during
///    the released interval restores to where it was before PiP rather than to
///    that manual arrangement — a stale-but-real frame, against a corner tile
///    that is provably wrong. Owner's rule: bounds are never destroyed.
///
///    NOTHING BLOCKS THE WATCHER THREAD. `SetWindowPos` and
///    `SetWindowPlacement` on a foreign window are SendMessage-class: they do
///    not return until the target's WndProc has run. The watcher thread is
///    the SOLE writer of `hook::FULLSCREEN_ACTIVE`, and a frozen writer is
///    PROBLEM 88's exact failure shape (flag stuck true → every shortcut
///    dead; stuck false → shortcuts fire inside a game). So the release's
///    Win32 work runs on a short-lived thread, the same medicine §6 applied
///    to the entry path. The watcher only ever does bookkeeping-class reads.
///
/// 9. FULLSCREEN-PRESERVING PiP — Space+Tab (PROBLEM 219, 2026-08-29).
///
///    THE PROBLEM SPACE+` CANNOT SOLVE. The owner watches a video fullscreen
///    in Brave and wants it cornered showing ONLY the video. Space+` cannot
///    do that by construction: it takes the window out of its fullscreen
///    state first, so every piece of browser chrome — tabs, address bar,
///    bookmarks — comes back with it. What he actually wants is the window
///    kept in its FULLSCREEN state while being moved and shrunk, so the page
///    still believes it is fullscreen, the video keeps filling the (now
///    small) window, and no browser UI reappears.
///
///    A SEPARATE KEY, NOT A CHANGE TO SPACE+`. The owner's explicit decision,
///    2026-08-29: *"I'm okay with the trade off. It's a new key. If I don't
///    like it, I can just not use it."* The trade-off is real and inherent —
///    a window kept in fullscreen has NO minimize or close buttons, because
///    fullscreen is precisely the state in which a window draws no chrome.
///    The 5th tap is the way out. Making Space+` behave this way would have
///    imposed that trade-off on every app he PiPs; **the key being separate
///    IS the opt-in**, which is why there is no setting and no browser
///    extension. `PipMode::Corner` is byte-for-byte today's Space+`.
///
///    WHY NOT THE BROWSER'S OWN DOCUMENT PICTURE-IN-PICTURE. Chromium has a
///    real document-PiP API that would give a proper always-on-top video
///    window with none of this Win32 work. It is unreachable from here: it
///    can only be called by code running INSIDE the page, which means an
///    extension the owner would have to install and maintain, a remote
///    debugging port left open on his browser, or UI automation clicking the
///    site's own PiP button — different in every site and broken by every
///    redesign. Rejected on 2026-08-29 for that reason, not on preference.
///
///    WHAT WAS MEASURED, AND IT IS THE WHOLE DESIGN (2026-08-29, a throwaway
///    Brave with its own `--user-data-dir`, never the owner's profile). A
///    genuinely fullscreen Chromium window — style `0x160B0000`, i.e. no
///    `WS_CAPTION` and no `WS_THICKFRAME`, `IsZoomed` false, covering
///    rcMonitor (0,0)-(2560,1600):
///
///      · A PLAIN `SetWindowPos(SWP_NOZORDER|SWP_NOACTIVATE)` to a 1280x800
///        corner tile returns TRUE with err=0 **and the window is back at
///        0,0 2560x1600 within 40 ms.** `MoveWindow` does the same. Chromium
///        reasserts its monitor bounds from `WM_WINDOWPOSCHANGING`. This is
///        trap 1, and it is real: today's Space+` cannot corner a fullscreen
///        Chromium window either, for exactly this reason.
///      · The SAME call plus **`SWP_NOSENDCHANGING`** moves the window and it
///        STAYS: measured through a full four-corner cycle and a restore, at
///        +150 ms and +1.65 s per corner and +4 s settled, style unchanged at
///        `0x160B0000` throughout — still fullscreen, never re-chromed, never
///        snapped back. `SWP_NOSENDCHANGING` suppresses the
///        `WM_WINDOWPOSCHANGING` the window uses to veto the resize, so the
///        veto never gets asked for.
///
///    That is why every placement on this path carries `SWP_NOSENDCHANGING`
///    (`move_flags`) and why this path never calls `animate_to`: `animate_to`
///    would (a) place with plain flags and be vetoed, and (b) run its
///    `IsZoomed → ShowWindow(SW_RESTORE)` precondition, which is the one call
///    guaranteed to drag a window out of the state this feature exists to
///    preserve.
///
///    THE VERIFICATION IS NOT OPTIONAL, even with the measurement in hand.
///    One Chromium build on one machine is not every app, and a window that
///    silently refuses is the failure the owner named: *never leave the user
///    with a window that is neither fullscreen nor cornered.* So the
///    placement thread re-reads the window `FS_VERIFY_MS` later and, if the
///    tile did not hold, **falls through to today's corner-snap** —
///    `fullscreen_pip` is cleared on the entry so cycling and the 5th tap use
///    ordinary flags, `animate_to` places it, and a toast says why. The
///    window is then in exactly the state Space+` would have left it in,
///    which is the floor this feature promises never to go below.
///
///    THE WATCHER EXEMPTION (trap 2). `release_enlarged` releases a PiP whose
///    window exceeds 75% of the work area or is `IsZoomed`. A window kept in
///    fullscreen sounds like it must trip that instantly — it does not, and
///    the measurement says why: after the move the window's REAL bounds are
///    the 1280x800 tile (25% of a 2560x1600 work area) and `IsZoomed` is
///    false. `should_release` has always measured actual bounds rather than
///    fullscreen state, so it correctly keeps the PiP. What DOES need an
///    exemption is the write-back: a fullscreen entry's `original_*` can be
///    the MONITOR RECT (it is whatever `measure_original_frame` saw, and on a
///    window whose showCmd is SW_SHOWNORMAL that is the fullscreen rect), and
///    handing that to `rcNormalPosition` would set the window's un-maximised
///    size to the whole screen. The exemption is unconditional rather than
///    conditional on the bounds, because the entry cannot tell the two cases
///    apart after the fact and the wrong answer is unrecoverable.
///    `release_disposition`
///    therefore never returns `Full` for a `fullscreen_pip` entry — topmost
///    is dropped, the entry is kept as the last copy of the bounds, and
///    Windows is never told anything false.
///
///    RESTORE SHARES THE MACHINERY BUT NOT THE MEASUREMENT (amended
///    2026-08-29 — this paragraph used to say the opposite, and the opposite
///    was the bug). The first cut reasoned: `measure_original_frame` for a
///    fullscreen window takes its `GetWindowRect` branch, because showCmd is
///    SW_SHOWNORMAL, so `original_*` IS the monitor rect and `was_maximized`
///    is false — therefore the ordinary `restore_window` already puts a
///    fullscreen window back to fullscreen. BOTH HALVES ARE FALSE on the
///    owner's Brave, and his log said so on the first entry it ever logged:
///
///        pip: entering PiP for hwnd 0x10c66
///             (restored frame 2558x1550 at (1,49), maximized=true)
///
///    `GetWindowPlacement` reports `showCmd == SW_SHOWMAXIMIZED` for a Brave
///    window that was maximised BEFORE it went fullscreen, so the measurement
///    takes the `rcNormalPosition` branch and stores the PRE-FULLSCREEN
///    WINDOWED frame, with `was_maximized` true. `restore_window` then ended
///    with `ShowWindow(SW_SHOWMAXIMIZED)` — and that is the whole defect:
///
///        working corners: fullscreen probe = true  (style 0x160B0000, zoomed=false)
///        after 5th tap:   fullscreen probe = false (style 0x170B0000, zoomed=true)
///
///    Same bounds, different STATE — `0x170B0000` is `0x160B0000 | WS_MAXIMIZE`.
///    `is_fullscreen_geometry` refuses a zoomed window, so the NEXT Space+Tab
///    probed false and correctly fell back to ordinary corner PiP. That is
///    what the owner reported as "the 4-corner loop works, then the 5th
///    fullscreen logic is broken". THE TELL WAS `zoomed=true`; the geometry
///    was right the whole time, which is why nothing that only checked
///    geometry could see it.
///
///    So the restore leg no longer INFERS the fullscreen state. The probe —
///    the one moment the window is known to be fullscreen — captures the
///    style, the ex-style and the rect into `PipEntry::fullscreen_state`, and
///    the 5th tap replays exactly that (`restore_plan` → `restore_window`):
///    un-maximise if something maximised it, put the style back, place the
///    captured rect with `SWP_NOSENDCHANGING`, and NEVER take the maximize
///    path. `original_*` and `was_maximized` are untouched and still correct
///    for what they describe — the window BEFORE fullscreen — and they are
///    what the documented fallback uses if the state refuses to go back.
///
///    MEASURED, not reasoned (2026-08-29, a throwaway Brave with its own
///    `--user-data-dir`, never the owner's profile — the same method that
///    found `SWP_NOSENDCHANGING`). Replaying the two restores against a
///    genuinely fullscreen window:
///
///      · TODAY'S restore (place `original_*`, then `SW_SHOWMAXIMIZED`):
///        style `0x170B0000`, `IsZoomed` true, showCmd 3. **The owner's log,
///        reproduced exactly.**
///      · THE FIX (un-maximise, reinstate style, place the captured rect with
///        `SWP_NOACTIVATE | SWP_NOSENDCHANGING`): style back to `0x160B0000`,
///        `IsZoomed` false, rect (0,0)-(2560,1600) — and unchanged 2.5 s
///        later, so Brave does not fight the return trip either.
///
///    A SECOND MEASURED CONDITION, recorded because it is a live trap rather
///    than a failure: a Brave window that was MAXIMISED and then sent F11
///    reaches fullscreen while KEEPING `WS_MAXIMIZE` — style `0x170B0000`,
///    `IsZoomed` true, showCmd 3, covering rcMonitor with no caption. It is
///    genuinely fullscreen and `is_fullscreen_geometry` says no, because it
///    refuses any zoomed window. Space+Tab therefore degrades to corner PiP
///    for it. That is the SAFE answer and it is not being changed here (the
///    zoomed guard is what keeps a merely-maximised window off the
///    suppressed-veto path), but it is why "Space+Tab did not preserve
///    fullscreen on that window" is not automatically this bug. Re-test
///    before treating it as one.
///
/// GENERALISE: WHEN A FEATURE PRESERVES A STATE, THE RESTORE MUST REPLAY THE
/// STATE IT CAPTURED, NOT RE-DERIVE IT FROM GEOMETRY. Bounds equality is not
/// state equality. The first cut's test asserted the 5th tap handed back the
/// right rect, and it passed for a restore that produced the wrong window.
///
/// 10. TWO SEPARATE CACHES, AND A TILE THAT SURVIVES BEING CLICKED
///     (2026-08-29 — PROBLEM 220, and PROBLEM 219 amendment 3).
///
///     Two owner reports on 1.0.93, both confirmed by measurement:
///
///      · *"it did behave oddly"* — Space+` and Space+Tab shared ONE
///        `PipEntry` per HWND, so they fought over one position index, one set
///        of original bounds and one sticky `fullscreen_pip` flag. The cache is
///        now keyed by `(hwnd, mode)` (`PipKey`), and a window is held by only
///        ONE key at a time: pressing the other key RESTORES the first key's
///        tile and then enters fresh (`takeover_victim`). See `PipKey` for why
///        the mode went into the key rather than into a second map.
///      · *"interacting with the tab pip just made it full screen at the first
///        tap"* — a fullscreen Chromium window re-runs its fullscreen layout
///        and reasserts the monitor rect ~16 ms after EVERY click into it, not
///        just the activating one. `SWP_NOSENDCHANGING` stops OUR move being
///        vetoed; it does nothing about the app moving itself later. Fixed by
///        an `EVENT_OBJECT_LOCATIONCHANGE` guard that re-asserts the tile — see
///        the §10 block above `arm_click_guard` for the full measurement, why
///        `EVENT_SYSTEM_FOREGROUND` was measured and rejected, and why this
///        cannot fight the user.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicIsize, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

/// Is this cache entry an ACTIVE PiP, or only the surviving record of one?
///
/// TASK 1 (2026-08-26, the owner's overrule of the first §7 cut). The
/// half-release used to be a bare `topmost_released: bool` that nothing but the
/// watcher consulted, so the ENGINE still saw an ordinary entry: the next tap
/// on the PiP key bumped `position_index` and cycled the window to the next
/// corner — while it was no longer topmost, because only ENTRY asserts topmost
/// and cycling never does. The user got a window hopping between corners from
/// behind everything else. Incoherent, and the owner's words for it were that
/// the entry must be marked released and treated as no longer actively PiP'd.
///
/// A named state, not a flag, because two different readers ask two different
/// questions of it — "is this still mine to cycle?" (the engine) and "have I
/// already dropped its topmost?" (the watcher) — and both are the same fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipState {
    /// Topmost is asserted, the window is a corner tile, and the next tap
    /// cycles it. The state every entry is born in.
    Active,
    /// The watcher has dropped HWND_TOPMOST because the window outgrew its
    /// corner tile, but the entry is deliberately RETAINED because the pre-PiP
    /// bounds could not be handed back to Windows (true fullscreen: not
    /// maximised, so `rcNormalPosition` is live geometry and rewriting it would
    /// move the window). **This entry is the last copy of those bounds and
    /// deleting it is the data loss §7 exists to forbid.**
    ///
    /// It is NOT an active PiP. The corner-cycling path must not touch it; the
    /// next tap re-enters from scratch (see `tap_for`). `restore_all()` still
    /// restores from it, and the watcher skips it instead of re-releasing it
    /// twice a second.
    Released,
}

/// §9 RESTORE LEG — the window's FULLSCREEN state, captured at entry.
///
/// Added 2026-08-29 after the owner reported that the 5th Space+Tab tap did
/// not give him true fullscreen back. Two assumptions in §9's first cut were
/// wrong, and BOTH are fixed by storing this:
///
///  1. *"A fullscreen entry's `original_*` IS the monitor rect."* Not always.
///     `measure_original_frame` reads `GetWindowPlacement`, and a Brave window
///     that was MAXIMISED before it went fullscreen still reports
///     `showCmd == SW_SHOWMAXIMIZED` — so the measurement takes the
///     `rcNormalPosition` branch and stores the window's PRE-FULLSCREEN
///     WINDOWED rect instead. The owner's own log, first entry:
///     `restored frame 2558x1550 at (1,49), maximized=true`. Placing that back
///     restores a WINDOW, not a fullscreen.
///  2. *"`was_maximized` is false for a fullscreen window."* Also not always,
///     for the same reason — and `restore_window` ended with
///     `ShowWindow(SW_SHOWMAXIMIZED)` whenever it was true, which is the whole
///     defect: it re-adds `WS_MAXIMIZE`, and `is_fullscreen_geometry` refuses a
///     zoomed window, so the NEXT tap probed false and fell back to corner PiP.
///
/// So the restore leg no longer infers the fullscreen state from bounds and a
/// show flag. It replays what was actually observed: this style, this ex-style,
/// this rect — the state the window was in at the instant the probe said yes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FullscreenState {
    /// `GWL_STYLE` at entry. Measured 0x160B0000 on a fullscreen Brave — the
    /// windowed control is 0x16CF0000, and the two differ in exactly
    /// `WS_CAPTION | WS_THICKFRAME`.
    pub style: u32,
    /// `GWL_EXSTYLE` at entry. Measured 0x00200000 and unchanged across the
    /// whole cycle, so this is a belt-and-braces capture rather than a known
    /// need — it costs one `GetWindowLongW` and closes the case where an app
    /// DOES move a bit that only the ex-style carries.
    pub ex_style: u32,
    /// The rect the window occupied while fullscreen, in SCREEN coordinates.
    /// This — not `original_*` — is what the 5th tap places back.
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipEntry {
    pub original_x: i32,
    pub original_y: i32,
    pub original_w: i32,
    pub original_h: i32,
    pub was_maximized: bool,
    /// 0=TopLeft, 1=TopRight, 2=BottomRight, 3=BottomLeft
    pub position_index: u8,
    /// The owning process at the moment PiP was entered. NATIVE_SAFETY rule 3:
    /// Windows recycles HWND values, and the release watcher acts on cached
    /// handles seconds-to-hours after they were stored — a recycled handle
    /// must be detectable BEFORE anything touches the window it now names.
    /// A pid mismatch is that detector (same-process recycling is handled by
    /// the explorer class check in `release_enlarged`).
    pub pid: u32,
    /// `tick_count` at the moment the entry was published — stamped AFTER the
    /// entry path's own blocking cross-process calls have returned, so it
    /// measures the age of the animation, not the age of a stalled syscall.
    /// The release watcher leaves entries younger than `ENTRY_SETTLE_MS`
    /// alone: on first entry the window can legitimately still be MAXIMISED
    /// for a while (the un-maximise is deferred to the animation thread, §6)
    /// and a watcher tick landing in that gap must not read "zoomed + in
    /// cache" as "the user maximised their PiP window".
    pub entered_at: u64,
    /// Identity. `entered_at` cannot serve: `tick_count` is GetTickCount64,
    /// ~16 ms granular, so two entries created inside one tick compare equal.
    ///
    /// The watcher decides on a SNAPSHOT and acts later, and between the two
    /// the engine can remove this entry (5th tap) and insert a brand-new one
    /// (next tap) for the same HWND. A bare `remove(&key)` cannot tell those
    /// apart — it hands back the NEW entry and the watcher releases a PiP the
    /// user created milliseconds ago, inside the very settle grace that exists
    /// to protect it. Claiming only when the serial still matches closes that.
    pub serial: u64,
    /// Active PiP, or a released entry kept only for its bounds? See
    /// `PipState`. Set once by the watcher's half-release, and reset to
    /// `Active` by a re-entry — which REUSES `original_*` rather than
    /// re-measuring, because at that moment the window is showing its
    /// fullscreen rect and measuring it would overwrite the only surviving
    /// copy of the pre-PiP bounds with the very geometry PiP is supposed to
    /// take it out of.
    pub state: PipState,
    /// §9 — was this entry created by Space+Tab on a window that was
    /// genuinely FULLSCREEN, and is that fullscreen state being preserved?
    ///
    /// Three things read it, and they are the whole of the feature:
    ///
    ///  1. `move_flags` — every placement for this entry carries
    ///     `SWP_NOSENDCHANGING`, without which Chromium reasserts its monitor
    ///     bounds within 40 ms (measured; see §9).
    ///  2. `release_disposition` — a fullscreen entry's `original_*` is the
    ///     MONITOR RECT, so it must never be written into `rcNormalPosition`.
    ///     `Full` is never returned for one.
    ///  3. `restore_plan` / `restore_window` — the 5th tap replays
    ///     `fullscreen_state` with the same suppressed-veto flag it was moved
    ///     out with, and never takes the `was_maximized` path. (It used to
    ///     place `original_*` and then re-maximise, which is what left the
    ///     window `zoomed=true` instead of fullscreen — see §9's restore leg.)
    ///
    /// **It is CLEARED, not set, by the fallback** (§9's verification): a
    /// window that refused to hold the tile is from that moment an ordinary
    /// corner PiP, and everything above must treat it as one. Space+`
    /// entries are born `false` and nothing ever sets them true, which is
    /// what keeps that key byte-for-byte unchanged.
    pub fullscreen_pip: bool,
    /// §9 RESTORE LEG — the fullscreen state this entry must be put BACK into,
    /// captured by the entry probe. `None` for every `Corner` entry, and for a
    /// fullscreen entry whose state could not be read.
    ///
    /// It is STICKY across a re-entry the same way `fullscreen_pip` is, and for
    /// a stronger reason: on a re-entry the probe normally succeeds and yields a
    /// fresh, truthful capture, but if it does not (the window was maximised in
    /// the released interval, say) the preserved copy is the last record of what
    /// fullscreen looked like for this window, and §8's rule is that a record of
    /// real geometry is never destroyed in favour of geometry PiP created.
    ///
    /// `restore_plan` is the ONLY reader. When it is `None` on an entry that
    /// still claims `fullscreen_pip`, restore degrades to today's
    /// `original_*` + `was_maximized` behaviour and says so — the owner's rule
    /// being that a window must never be left in a state that is neither
    /// fullscreen nor properly restored.
    pub fullscreen_state: Option<FullscreenState>,
}

/// Which of the two PiP keys is asking (§9).
///
/// `Corner` is Space+` — today's behaviour, every app, every case, and the
/// owner's instruction is that it stays that way. `FullscreenPreserving` is
/// Space+Tab, and it is a REQUEST, not a fact: it only becomes a fullscreen
/// PiP if the window turns out to be genuinely fullscreen when the tap lands
/// (`is_fullscreen_geometry`). If it is not, the request degrades to `Corner`
/// so the key is never dead — the owner's trap 4, decided 2026-08-29.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PipMode {
    /// Space+` — restore out of maximise/fullscreen first, then corner-snap.
    Corner,
    /// Space+Tab — do NOT leave fullscreen; move and shrink in place.
    FullscreenPreserving,
}

impl PipMode {
    /// The OTHER key. Used only by the takeover check (§10).
    #[cfg(windows)]
    fn other(self) -> PipMode {
        match self {
            PipMode::Corner => PipMode::FullscreenPreserving,
            PipMode::FullscreenPreserving => PipMode::Corner,
        }
    }

    /// How the log and the takeover toast name this key.
    #[cfg(windows)]
    fn key_name(self) -> &'static str {
        match self {
            PipMode::Corner => "Space+`",
            PipMode::FullscreenPreserving => "Space+Tab",
        }
    }
}

/// §10 (PROBLEM 220) — the cache key: a window AND the key that cornered it.
///
/// THE OWNER'S REPORT, 2026-08-29: *"it did behave oddly"*, after being warned
/// that Space+` and Space+Tab shared one `PipEntry` map. His instruction:
/// *"make separate pip cache, is it possible?"*
///
/// WHY THE MODE IS IN THE KEY RATHER THAN IN A SECOND MAP. Two maps give the
/// same separation, and they were rejected for one reason: EVERY consumer of
/// this cache has to see ALL of it, and with two containers each of them is one
/// forgotten line away from a silent half-failure that nothing would report.
/// `restore_all()` draining only one map leaves windows pinned topmost at
/// quarter size with the app gone — the exact PROBLEM 167 orphan this file
/// exists to prevent. `release_enlarged()` scanning only one leaves the other
/// feature's windows stuck on top forever. `prune_dead` pruning only one leaves
/// dead HWNDs to be inherited by a recycled handle (NATIVE_SAFETY rule 3). With
/// the mode in the key there is ONE map, so a drain is a drain, a scan is a
/// scan, and a prune is a prune — none of them can be taught about half the
/// state, and a third PiP mode would inherit all three for free.
///
/// THE MODE HERE IS THE KEY THAT WAS PRESSED, NOT THE BEHAVIOUR THAT RESULTED.
/// A Space+Tab tap on a window that turns out NOT to be fullscreen degrades to
/// corner behaviour (`fullscreen_pip: false`) but still lives under
/// `FullscreenPreserving`, because "which key owns this window" is what the
/// user experiences and what the takeover rule below is stated in terms of.
/// The BEHAVIOUR is still decided by `PipEntry::fullscreen_pip`, exactly as
/// before.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PipKey {
    pub hwnd: isize,
    pub mode: PipMode,
}

pub type PipCache = Arc<Mutex<HashMap<PipKey, PipEntry>>>;

/// ONE cache for the process.
///
/// It used to be a fresh map per `EngineState`, which was fine while the only
/// reader was the engine. `restore_all()` runs from the Tauri exit handler,
/// which has no engine handle at all — and a restore-on-exit that cannot see
/// the entries is not a restore. Sharing one map keeps `new_cache()`'s
/// signature and gives shutdown something to read. `release_enlarged()` (the
/// fullscreen watcher's reader, §7) reads it the same way.
///
/// §10: still one map, now keyed by `(hwnd, mode)` — see `PipKey` for why the
/// separation went into the key rather than into a second container.
fn global_cache() -> &'static PipCache {
    static CACHE: OnceLock<PipCache> = OnceLock::new();
    CACHE.get_or_init(|| Arc::new(Mutex::new(HashMap::new())))
}

/// Handle to the shared PiP cache.
pub fn new_cache() -> PipCache {
    global_cache().clone()
}

/// Bumped by every new animation. A running spring loop that finds the counter
/// has moved past its own ticket snaps to ITS OWN target and stops, so a
/// superseded flight neither fights the new one nor abandons its window
/// half-way to a corner.
static ANIM_GEN: AtomicU64 = AtomicU64::new(0);

/// Bumped by restore and release — the paths after which the window is no
/// longer PiP's to move AT ALL. A flight that observes a bump stops WITHOUT
/// placing. This is deliberately different from the ANIM_GEN supersede above:
/// snap-to-own-target is right when a NEWER TAP took over (its flight
/// immediately re-drives the window), but after a restore/release that same
/// parting snap could land AFTER the restore's `SetWindowPos` and quietly
/// drag the window back to a corner — undoing a restore with nothing left in
/// the cache to fix it. A global epoch, not per-window: a bump also halts an
/// innocent concurrent flight for a DIFFERENT PiP window mid-air (it simply
/// stops where it is, un-placed). With the cache normally holding 0–1 entries
/// that is theoretical, and correctness of the restored window's final
/// position outranks animation polish for a hypothetical second one.
static ANIM_CANCEL: AtomicU64 = AtomicU64::new(0);

/// Monotonic identity for cache entries — see `PipEntry::serial`.
static PIP_SERIAL: AtomicU64 = AtomicU64::new(0);

/// The HWND whose deferred un-maximise is currently in flight, or 0.
///
/// `ENTRY_SETTLE_MS` alone is a TIMER, and the thing it is guarding against
/// is a cross-process `ShowWindow` with no upper bound on how long it takes.
/// A wedged pump can hold the window MAXIMISED past any fixed grace, after
/// which the watcher reads "zoomed + in cache" as "the user maximised their
/// PiP window" and releases a PiP that never got to happen. This marker is
/// the fact rather than the estimate: while it names a window, that window's
/// zoomed-ness is ours, not the user's, and the watcher skips it.
///
/// One slot, because the cache normally holds 0-1 entries. Two overlapping
/// flights for DIFFERENT windows would have the second overwrite the first's
/// marker; the loser simply falls back to `ENTRY_SETTLE_MS`, which is where
/// it was before this existed. Cleared by an RAII guard so an early return
/// cannot strand it.
#[cfg(windows)]
static UNMAX_IN_FLIGHT: AtomicIsize = AtomicIsize::new(0);

/// §7 — how long after entry `release_enlarged` must keep its hands off an
/// entry, in ms. Budget: the entry spring flight runs ≤ 960 ms and the
/// watcher ticks every 500 ms — 2000 ms covers both with slack. A real "user
/// maximised their PiP window" takes seconds of human action, so the grace
/// costs at most one extra tick of the buggy old behaviour. It is NOT asked
/// to cover the deferred un-maximise, whose duration is unbounded;
/// `UNMAX_IN_FLIGHT` covers that with a fact instead of a guess.
#[cfg(windows)]
const ENTRY_SETTLE_MS: u64 = 2000;

/// What a tap on the PiP key means, given what the cache already holds for
/// this window.
///
/// Module scope — it used to be declared inside `toggle_pip_win32`, where the
/// decision could not be tested without a real window. TASK 1 gave it a fourth
/// thing to get right, so it moved out.
#[cfg(windows)]
#[derive(Debug, Clone, PartialEq, Eq)]
enum Tap {
    /// Fly to corner `n`. ACTIVE entries only.
    Cycle(u8),
    /// The 5th tap: put the window back where it came from and let go of it.
    Restore(PipEntry),
    /// Enter PiP. `Some(preserved)` means this window has a RELEASED entry
    /// (§8) whose `original_*` bounds must be carried into the new entry
    /// VERBATIM — re-measuring the window here is the data loss §8 forbids.
    Enter(Option<PipEntry>),
}

/// Pure (TASK 1): decide what this tap means and do the cache bookkeeping for
/// it, without touching any window.
///
/// The released arm is the whole point. A `PipState::Released` entry is the
/// surviving record of a PiP, not a live one — the window is not topmost any
/// more — so cycling it to the next corner produces a window that hops around
/// from BEHIND everything else. It is treated as absent for cycling and the
/// tap becomes a fresh entry, carrying the preserved bounds forward.
///
/// THE PID CHECK IS ONLY ON THE RELEASED ARM, deliberately. A released entry
/// is long-lived by construction (it survives until the 5th tap, a maximise,
/// or app exit — hours, potentially), and it is the ONE path that skips
/// measuring the live window and trusts stored numbers instead. Trusting them
/// for a RECYCLED handle would tile a stranger's window and later "restore" it
/// to bounds it never had — NATIVE_SAFETY rule 3, and invisible until the user
/// un-maximises. An active entry is re-driven by the user every few seconds
/// and re-measures nothing, so it is left exactly as it was.
#[cfg(windows)]
fn tap_for(map: &mut HashMap<PipKey, PipEntry>, key: PipKey, pid: u32) -> Tap {
    let Some(existing) = map.get(&key).cloned() else {
        return Tap::Enter(None);
    };

    if existing.state == PipState::Released {
        // BOTH pids must be known AND different before this counts as a
        // recycled handle. A zero on either side means "could not tell", and
        // "could not verify" is never treated as "verified different" here —
        // the same polarity rule `browser_profiles::profile_arg_for` spells
        // out, and doubly so on this path, where the wrong answer THROWS AWAY
        // the last copy of the window's real bounds.
        let recycled = pid != 0 && existing.pid != 0 && pid != existing.pid;
        if !recycled {
            return Tap::Enter(Some(existing));
        }
        log::warn!(
            "pip: the released {} entry for hwnd {:#x} was stored for pid {} but the window \
             now belongs to pid {pid} — the handle was recycled. Dropping the stale bounds \
             and entering PiP fresh, measuring the window that is actually there.",
            key.mode.key_name(),
            key.hwnd,
            existing.pid
        );
        map.remove(&key);
        return Tap::Enter(None);
    }

    let entry = map.get_mut(&key).expect("present — just cloned from it");
    entry.position_index += 1;
    if entry.position_index >= 4 {
        let entry = entry.clone();
        map.remove(&key);
        Tap::Restore(entry)
    } else {
        Tap::Cycle(entry.position_index)
    }
}

/// Toggle PiP on the current foreground window (Space+`).
/// Returns a notification message string.
pub fn toggle_pip(cache: &PipCache) -> String {
    #[cfg(windows)]
    unsafe {
        toggle_pip_win32(cache, PipMode::Corner)
    }
    #[cfg(not(windows))]
    {
        let _ = cache;
        String::from("PiP not supported on this platform")
    }
}

/// §9 — Space+Tab: FULLSCREEN-PRESERVING PiP.
///
/// Corner the foreground window WITHOUT taking it out of fullscreen, so a
/// fullscreen video keeps filling the shrunken window and no browser chrome
/// comes back. See §9 in the header for the measurement this rests on, why it
/// is a separate key from Space+`, and why the browser's own document-PiP API
/// was rejected.
///
/// Shares the MACHINERY with `toggle_pip` — one `tap_for`, one `PipEntry`
/// shape, one `restore_window` — but NOT the state. Since PROBLEM 220 the two
/// keys own separate namespaces of the cache (`PipKey`), so neither can see the
/// other's bounds, corner index or fullscreen capture, and a window held by one
/// key is TAKEN OVER (restored first, then entered afresh) when the other key
/// is pressed on it. Before that they shared one entry per HWND, which the
/// owner reported as *"it did behave oddly."*
pub fn toggle_fullscreen_pip(cache: &PipCache) -> String {
    #[cfg(windows)]
    unsafe {
        toggle_pip_win32(cache, PipMode::FullscreenPreserving)
    }
    #[cfg(not(windows))]
    {
        let _ = cache;
        String::from("Fullscreen PiP not supported on this platform")
    }
}

#[cfg(windows)]
unsafe fn toggle_pip_win32(cache: &PipCache, requested: PipMode) -> String {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::{
        Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST},
        UI::WindowsAndMessaging::{
            GetCursorPos, GetForegroundWindow, GetWindowThreadProcessId, IsIconic, SetWindowPos,
            ShowWindow, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_RESTORE,
        },
    };

    // Function-entry timestamp for the first-entry timing line. Captured on
    // every tap because the branch is not known yet; GetTickCount64 is a
    // handful of nanoseconds, so corner-cycling pays nothing measurable.
    let t_entry = crate::hook::tick_count_pub();

    let hwnd = GetForegroundWindow();
    if hwnd.0.is_null() {
        log::warn!("pip: no foreground window — nothing to toggle");
        return String::new();
    }

    // Never act on our own dashboard or overlay. Stripping/moving Spaceadom's
    // own window from inside Spaceadom is not a feature anyone asked for, and
    // the overlay is click-through and NoActivate so it should never be
    // foreground in the first place — if it somehow is, that is a bug to log,
    // not a window to tile.
    if is_own_window(hwnd) {
        log::warn!("pip: the foreground window belongs to Spaceadom — refusing to PiP ourselves");
        return "PiP needs another app's window".to_string();
    }

    // NATIVE_SAFETY §1 rows 1 and 2, on the NEW key only (§9).
    //
    // Scope, deliberately. `PipMode::Corner` is Space+`, and the owner's
    // instruction is that it is "completely unchanged. Today's behaviour,
    // every app, every case." Adding the empty-title skip to it would be a
    // behaviour change he could actually hit — a fullscreen media player with
    // no window title is a plausible Space+` target, and refusing it would be
    // a regression in a feature he uses today. So the guard runs for
    // Space+Tab, which has no existing behaviour to preserve, and the gap on
    // Space+` is REPORTED rather than silently closed. That is a decision for
    // the owner, not a thing to slip into a diff about another key.
    if requested == PipMode::FullscreenPreserving {
        if let Some(reason) = shell_safety_refusal(hwnd) {
            log::warn!(
                "fs-pip: refusing hwnd {:#x} — {reason} (NATIVE_SAFETY). This app once broke \
                 the owner's touchpad by acting on explorer shell windows; when in doubt the \
                 rule is skip the window, not act on it.",
                hwnd.0 as isize
            );
            return "🚫 Not a window Spaceadom may move".to_string();
        }
    }

    // PROBLEM 167 — repair a window the OLD PiP left stripped. This is a
    // legacy path only: nothing this build does can create one.
    //
    // The membership check holds the cache lock for exactly one HashMap
    // lookup. It used to be one expression with `looks_orphaned`, which — by
    // Rust's temporary-scope rule — kept the lock guard alive across all of
    // `looks_orphaned`'s Win32 queries. Harmless then, but the fullscreen
    // watcher now shares this lock (§7), so no Win32 call runs under it.
    //
    // §10: "tracked" means tracked by EITHER key. A window the other key is
    // holding is emphatically not an orphan from a 2026-08-24 build.
    let already_tracked = {
        let map = cache.lock().unwrap_or_else(|p| p.into_inner());
        let h = hwnd.0 as isize;
        map.contains_key(&PipKey {
            hwnd: h,
            mode: PipMode::Corner,
        }) || map.contains_key(&PipKey {
            hwnd: h,
            mode: PipMode::FullscreenPreserving,
        })
    };
    if !already_tracked && looks_orphaned(hwnd) {
        rescue_orphan(hwnd);
        return "🔧 Window frame repaired — press again for PiP".to_string();
    }

    // A minimised window has a meaningless rect; restore it before measuring.
    // (This ShowWindow CAN block on the target app, but a minimised window
    // being foreground is a rare corner already — the entry-lag work in §6 is
    // about the common maximised case, not this one.)
    if IsIconic(hwnd).as_bool() {
        let _ = ShowWindow(hwnd, SW_RESTORE);
    }

    // Corners come from the CURSOR's monitor, on purpose — see §4 in the
    // header. This is what lets PiP throw a window onto the screen you are
    // pointing at.
    let mut cursor_pos = POINT::default();
    GetCursorPos(&mut cursor_pos).ok();
    let monitor = MonitorFromPoint(cursor_pos, MONITOR_DEFAULTTONEAREST);
    let mut mon_info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if !GetMonitorInfoW(monitor, &mut mon_info).as_bool() {
        log::warn!("pip: GetMonitorInfoW failed for hwnd {:?}", hwnd);
        return String::new();
    }

    // PiP is positioned against the WORK area (rcWork, excludes the taskbar),
    // not the raw monitor bounds — so rcMonitor's dimensions are unused.
    let wa = mon_info.rcWork;
    let mon_w = wa.right - wa.left;
    let mon_h = wa.bottom - wa.top;

    // PiP size = 50% × 50% of the work area (i.e. 25% of its area)
    let pip_w = mon_w / 2;
    let pip_h = mon_h / 2;

    let hwnd_key = hwnd.0 as isize;

    // Read the owning process BEFORE the lock. Bookkeeping-class (it reads the
    // window-manager's own table and never dispatches into the target's
    // thread), so it is cheap and cannot block — but `tap_for` needs it to
    // decide whether a RELEASED entry's stored bounds may still be trusted,
    // and no Win32 call may run under the cache lock (TASK 4, below).
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));

    // TASK 4 (2026-08-26) — the mutex is held ONLY around map operations.
    // It used to be held across IsZoomed / ShowWindow / GetWindowRect in the
    // first-entry branch. That was benign while the engine (which dispatches
    // serially) was the only reader, but the fullscreen watcher (§7) now
    // reads this cache every 500 ms from its own thread, and a lock held
    // across a call that can block on a FOREIGN process would let one wedged
    // Electron app stall the watcher too. The decision below is taken under
    // one short lock; every Win32 call happens with the lock released. The
    // check-then-insert gap this opens for the Enter arm is safe: the engine
    // is the only INSERTER and it dispatches PiP taps serially, while the
    // watcher only REMOVES entries — so nothing can insert this hwnd between
    // our check and our insert.
    //
    // §10 — the key that was PRESSED is half the cache key. `requested`, not
    // `effective`: a Space+Tab tap that degrades to corner behaviour still
    // belongs to Space+Tab's namespace (see `PipKey`).
    let key = PipKey {
        hwnd: hwnd_key,
        mode: requested,
    };

    let (victim, tap) = {
        let mut cache_lock = cache.lock().unwrap_or_else(|p| p.into_inner());

        // Drop entries whose windows are gone BEFORE looking ours up. Windows
        // recycles HWND values, so a dead entry left in the map can be
        // inherited by an unrelated new window — which then starts its life
        // at "corner 2" or gets "restored" to a stranger's geometry. That is
        // a large part of "doesn't go to the 4 corners properly".
        // (`IsWindow` inside is window-manager bookkeeping — it never
        // dispatches into the target's thread, so it may run under the lock.)
        prune_dead(&mut cache_lock);

        // §10 — claim the window off the OTHER key BEFORE deciding what this
        // tap means. Both operations are pure map work under the one short
        // lock; the victim's restore happens below, with the lock released.
        let victim = takeover_victim(&mut cache_lock, hwnd_key, requested);
        let tap = tap_for(&mut cache_lock, key, pid);
        (victim, tap)
    };

    // §10 — THE TAKEOVER, executed. The other key's tile is put back the way
    // that feature found it, and only then does this tap proceed. Deliberately
    // BEFORE the `Tap::Enter` arm's `measure_original_frame`: without the
    // restore, this entry's `original_*` would be a measurement of the other
    // feature's corner tile, and the 5th tap would "restore" the user's window
    // to a quarter of the screen with nothing left that knows better.
    if let Some(victim) = victim {
        // NATIVE_SAFETY rule 3, and the STRICT polarity because this branch
        // ACTS on the window rather than merely reading stored numbers: a
        // recycled handle must be dropped without being touched. (`tap_for`'s
        // released arm uses the softer "both known and different" rule because
        // the wrong answer there only costs a re-measure; the wrong answer here
        // moves a stranger's window.)
        if pid != 0 && victim.pid != 0 && pid == victim.pid {
            log::info!(
                "pip: TAKEOVER — hwnd {hwnd_key:#x} is currently held by {} and {} was just \
                 pressed. A window is only ever held by ONE of the two keys (PROBLEM 220), so \
                 the {} tile is being restored to its original frame first and this tap then \
                 enters PiP fresh. Two live entries for one window is what the owner reported \
                 as \"it did behave oddly\".",
                requested.other().key_name(),
                requested.key_name(),
                requested.other().key_name(),
            );
            restore_window(hwnd, &victim);
        } else {
            log::warn!(
                "pip: hwnd {hwnd_key:#x} carried a {} entry stored for pid {} but the window \
                 now belongs to pid {pid} — the handle was recycled. Dropping that entry \
                 WITHOUT touching the window (NATIVE_SAFETY rule 3).",
                requested.other().key_name(),
                victim.pid
            );
        }
    }

    match tap {
        Tap::Restore(entry) => {
            // 5th tap: put it back the way it was found and let go of it.
            // WHICH restore that is comes off the ENTRY, not off the key —
            // a fullscreen entry replays its captured fullscreen state
            // (`restore_plan`), a corner entry gets `original_*` and its
            // maximised state — so either key's 5th tap does the right thing
            // for the window it is acting on.
            log::info!(
                "pip: restoring hwnd {hwnd_key:#x} to its original frame (fullscreen_pip={})",
                entry.fullscreen_pip
            );
            restore_window(hwnd, &entry);
            if entry.fullscreen_pip {
                "↩️ Fullscreen Restored".to_string()
            } else {
                "↩️ Frame Restored".to_string()
            }
        }

        Tap::Cycle(idx) => {
            // The MODE COMES FROM THE ENTRY, not from the key that was
            // pressed — and since PROBLEM 220 that entry is this key's OWN
            // entry, looked up under `key` rather than under a bare HWND. A
            // tap of the other key never reaches here at all: it takes the
            // window over instead (see the TAKEOVER above). The lookup stays
            // entry-driven because `fullscreen_pip` can have been CLEARED by
            // the placement fallback, and a cycle must honour that demotion
            // rather than re-deciding from the key that was pressed.
            let fs = cache
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .get(&key)
                .map(|e| (e.fullscreen_pip, e.serial))
                .unwrap_or((false, 0));
            let (x, y) = corner_position(idx, wa.left, wa.top, pip_w, pip_h, mon_w, mon_h);
            log::info!(
                "pip: hwnd {hwnd_key:#x} → corner {idx} at ({x},{y}) {pip_w}x{pip_h} \
                 (fullscreen_pip={})",
                fs.0
            );
            if fs.0 {
                place_fullscreen_preserving(hwnd, x, y, pip_w, pip_h, cache.clone(), key, fs.1);
                fullscreen_corner_label(idx)
            } else {
                animate_to(hwnd, x, y, pip_w, pip_h);
                corner_label(idx)
            }
        }

        Tap::Enter(preserved) => {
            // ── Entry into PiP ───────────────────────────────────────────
            //
            // TWO WAYS IN, and only one of them measures the window.
            //
            // `preserved == None` is a genuine first entry: measure (§6).
            //
            // `preserved == Some(entry)` is a RE-ENTRY over a released entry
            // (§8). The pre-PiP bounds are already in hand, and the window is
            // currently displaying geometry that PiP itself is responsible
            // for — the fullscreen rect it was released into, or the corner
            // tile Windows restores it to when the user leaves fullscreen.
            // Measuring either and storing it as `original_*` would overwrite
            // the last surviving copy of the real bounds, so the 5th tap
            // would "restore" the window to a quarter-screen tile with
            // nothing left anywhere that knows better. Reuse, never
            // re-measure.
            let reentry = preserved.is_some();

            // §9 — WHICH KIND OF PiP IS THIS ENTRY? Decided here and nowhere
            // else, because this is the only tap that creates an entry, and
            // everything downstream (`move_flags`, `release_disposition`,
            // `restore_window`) reads the answer back off the entry rather
            // than re-deciding it.
            //
            // `FullscreenPreserving` is a REQUEST. It only becomes fact if
            // the window is genuinely fullscreen RIGHT NOW — trap 4, and the
            // owner's suggested answer to it: a key that silently no-ops is
            // worse than one that does something sensible, so a request
            // against a non-fullscreen window degrades to an ordinary corner
            // PiP and the toast says so. The probe is bookkeeping-class only
            // (GetWindowLong, GetWindowRect, GetMonitorInfo, IsZoomed) — it
            // never dispatches into the target's message pump.
            let mut fell_back_not_fullscreen = false;
            // The probe's CAPTURE, not just its verdict (2026-08-29, the
            // restore leg). This is the only instant at which the window is
            // known to be fullscreen, so it is the only honest place to copy
            // the style/ex-style/rect that the 5th tap has to put back.
            let mut probed_state: Option<FullscreenState> = None;
            let effective = match requested {
                PipMode::Corner => PipMode::Corner,
                PipMode::FullscreenPreserving => {
                    if let Some(fs) = fullscreen_probe(hwnd) {
                        probed_state = Some(fs);
                        PipMode::FullscreenPreserving
                    } else {
                        fell_back_not_fullscreen = true;
                        log::info!(
                            "fs-pip: hwnd {hwnd_key:#x} is NOT fullscreen (it has a caption or \
                             a resize frame, or it does not cover its monitor), so there is no \
                             fullscreen state to preserve. Falling back to the ordinary \
                             corner-snap Space+` does, which is the whole point of the \
                             fallback: this key is never dead."
                        );
                        PipMode::Corner
                    }
                }
            };

            let t_before_read = crate::hook::tick_count_pub();

            let (ox, oy, ow, oh, maximized) = if let Some(prev) = &preserved {
                // The numbers themselves are printed by the shared entry line
                // just below, so they are deliberately NOT repeated here —
                // which also keeps the structural test in this file honest:
                // the only place these field names appear in this branch is
                // the tuple that actually stores them.
                log::info!(
                    "pip: hwnd {hwnd_key:#x} still carries a RELEASED entry (its always-on-top \
                     was dropped when the window went true-fullscreen, and the entry was kept \
                     because it is the last copy of the pre-PiP bounds). Re-entering PiP FRESH \
                     — corner 0, topmost re-asserted — and REUSING those preserved bounds \
                     rather than measuring the window, which is showing PiP's own geometry \
                     right now."
                );
                (
                    prev.original_x,
                    prev.original_y,
                    prev.original_w,
                    prev.original_h,
                    prev.was_maximized,
                )
            } else {
                match measure_original_frame(hwnd, hwnd_key) {
                    Some(m) => m,
                    // Nothing measurable and nothing preserved: there is no
                    // honest `original_*` to store, and an entry with invented
                    // bounds is worse than no PiP at all.
                    None => return String::new(),
                }
            };
            let t_after_read = crate::hook::tick_count_pub();

            log::info!(
                "pip: {} PiP for hwnd {hwnd_key:#x} (restored frame {ow}x{oh} at \
                 ({ox},{oy}), maximized={maximized})",
                if reentry { "re-entering" } else { "entering" }
            );

            // Stay-on-top is the ONE thing PiP still changes about the
            // window, and it is the point of the feature. Every exit route
            // clears it again: the 5th tap, `rescue_orphan`, `restore_all`
            // at shutdown, and `release_enlarged` (§7). No SWP_FRAMECHANGED —
            // nothing about the frame is being altered any more, and asking
            // for a frame recalculation we do not need is how you make a
            // Chromium window flicker.
            //
            // THIS RUNS BEFORE THE CACHE INSERT, and the order is load-bearing
            // (2026-08-26, review round 2). It is a cross-process z-order
            // change: it SendMessages WM_WINDOWPOSCHANGING/CHANGED into the
            // target's pump and does not return until that pump has run it —
            // the same class of call §6 moved off this thread. When the entry
            // was published FIRST, a slow pump left a cache entry whose
            // `entered_at` was already ageing while the window was still
            // maximised and no flight had even been spawned: the watcher's
            // settle grace expired, it released the entry, wrote the true
            // bounds into rcNormalPosition — and then the engine's flight,
            // which snapshots ANIM_CANCEL only after this call returns, never
            // saw the cancel, un-maximised the window and dragged it to a
            // corner in its NORMAL state, which made Windows overwrite that
            // freshly-repaired rcNormalPosition with the corner tile. Last
            // copy gone, both in the cache and in the OS.
            //
            // Publishing the entry AFTER this call fixes both halves at once:
            // `entered_at` starts when the animation starts, and the flight's
            // ANIM_CANCEL snapshot is now microseconds behind the insert
            // instead of seconds. The window is topmost-but-untracked for
            // those microseconds, which no other thread can observe (the
            // watcher only ever looks at cached entries).
            let t_before_topmost = crate::hook::tick_count_pub();
            let _ = SetWindowPos(
                hwnd,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
            let t_after_topmost = crate::hook::tick_count_pub();

            // Hoisted out of the struct literal: the placement thread needs
            // the same serial, so that a flight can tell "the entry I was
            // spawned for" from "whatever is under this key now" — the same
            // discipline `claim_for_release` uses on the watcher side.
            let new_serial = PIP_SERIAL.fetch_add(1, Ordering::SeqCst) + 1;

            {
                let mut cache_lock = cache.lock().unwrap_or_else(|p| p.into_inner());
                cache_lock.insert(
                    key,
                    PipEntry {
                        original_x: ox,
                        original_y: oy,
                        original_w: ow,
                        original_h: oh,
                        was_maximized: maximized,
                        position_index: 0,
                        pid,
                        entered_at: crate::hook::tick_count_pub(),
                        // A NEW serial even on re-entry, and it is load-bearing:
                        // the watcher may be holding a snapshot of the released
                        // entry right now, and `claim_for_release` /
                        // `reentered_since_claim` both key off the serial. A
                        // re-entry that kept the old one would let a stale
                        // claim drop the topmost this tap just re-asserted.
                        serial: new_serial,
                        state: PipState::Active,
                        // STICKY, and that is the point. `preserved` can be a
                        // released fullscreen entry whose `original_*` is a
                        // MONITOR RECT; if this re-entry cleared the flag,
                        // `release_disposition` could later hand that monitor
                        // rect to `rcNormalPosition` and set the window's
                        // un-maximised size to the whole screen. Once the
                        // stored bounds are fullscreen bounds they must stay
                        // marked as such for as long as the entry lives.
                        fullscreen_pip: effective == PipMode::FullscreenPreserving
                            || preserved.as_ref().is_some_and(|p| p.fullscreen_pip),
                        // STICKY the same way, and for the same reason. A
                        // fresh capture wins when the probe got one — it
                        // describes the window as it is right now. When it did
                        // not (this tap was Space+`, or the window was
                        // maximised during the released interval), the
                        // preserved copy is the last record of what fullscreen
                        // looked like for this window, and §8's rule is that a
                        // record of REAL geometry is never dropped in favour of
                        // geometry PiP itself created.
                        fullscreen_state: probed_state
                            .or_else(|| preserved.as_ref().and_then(|p| p.fullscreen_state)),
                    },
                );
            }

            let (x, y) = corner_position(0, wa.left, wa.top, pip_w, pip_h, mon_w, mon_h);
            let t_before_anim = crate::hook::tick_count_pub();
            if effective == PipMode::FullscreenPreserving {
                place_fullscreen_preserving(hwnd, x, y, pip_w, pip_h, cache.clone(), key, new_serial);
            } else {
                animate_to(hwnd, x, y, pip_w, pip_h);
            }
            let t_after_anim = crate::hook::tick_count_pub();

            // §6 — the first-entry timing line, and ONLY first-entry: corner
            // cycling stays instrumentation-free. This exists because the old
            // "entering PiP" line fired AFTER the blocking restore and BEFORE
            // the topmost call, so it could not say which call ate the time.
            // Read it as: whichever number is large is the slow call. All
            // values come from tick_count (GetTickCount64, ~16 ms granularity
            // — coarse, but the lag being hunted is tens of ms and up). The
            // exe name costs one OpenProcess round-trip and is resolved AFTER
            // the animation is already flying, so it delays nothing visible.
            // (A re-entry reports placement-read≈0 by construction — it reuses
            // the preserved bounds instead of asking Windows for any — so the
            // flag is printed rather than left to be inferred from a zero.)
            let exe = crate::hook::exclusions::process_stem_for_pid(pid);
            log::info!(
                "pip: entry-timing exe={exe} reentry={reentry}: placement-read={}ms topmost={}ms \
                 animate-spawn={}ms entry-to-animate={}ms (un-maximise, when needed, runs on \
                 the animation thread and logs its own duration)",
                t_after_read.saturating_sub(t_before_read),
                t_after_topmost.saturating_sub(t_before_topmost),
                t_after_anim.saturating_sub(t_before_anim),
                t_before_anim.saturating_sub(t_entry),
            );

            // Three outcomes, three different things the user needs told.
            // The fallback one is not decoration: it is the difference
            // between "this key is broken" and "that window was not
            // fullscreen, so you got the ordinary corner PiP".
            match (effective, fell_back_not_fullscreen) {
                (PipMode::FullscreenPreserving, _) => "📺 Fullscreen PiP: Top-Left".into(),
                (PipMode::Corner, true) => "📐 Not fullscreen — corner PiP".into(),
                (PipMode::Corner, false) => "📺 PiP: Top-Left".into(),
            }
        }
    }
}

/// Measure the frame a window should be restored to when PiP lets go of it,
/// as `(x, y, w, h, was_maximized)` in SCREEN coordinates. `None` means the
/// window could not be measured at all, which is a refusal to enter PiP — an
/// entry carrying invented bounds is worse than no PiP.
///
/// Extracted from the entry arm for TASK 1: there are now two ways into PiP
/// and only ONE of them may measure. A re-entry over a released entry (§8)
/// must NOT call this — the window is displaying PiP's own geometry at that
/// moment, and storing it would destroy the last copy of the real bounds.
/// Keeping the measurement in its own function is what makes that call site
/// visible instead of buried in a 60-line arm.
///
/// §6 — this measures WITHOUT touching the window. `GetWindowPlacement` yields
/// both the show state (showCmd) and the RESTORED bounds (rcNormalPosition)
/// while the window is still maximised, so the old blocking
/// `ShowWindow(SW_RESTORE)`-then-`GetWindowRect` sequence is gone from the
/// engine thread. The un-maximise itself is handed to the animation thread,
/// which re-checks `IsZoomed` rather than being told.
#[cfg(windows)]
unsafe fn measure_original_frame(
    hwnd: windows::Win32::Foundation::HWND,
    hwnd_key: isize,
) -> Option<(i32, i32, i32, i32, bool)> {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowPlacement, GetWindowRect, IsZoomed, SW_SHOWMAXIMIZED, WINDOWPLACEMENT,
    };

    let mut wp = WINDOWPLACEMENT {
        length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
        ..Default::default()
    };
    let have_placement = GetWindowPlacement(hwnd, &mut wp).is_ok();
    let maximized = if have_placement {
        // showCmd is u32 in windows 0.58; SW_SHOWMAXIMIZED is a
        // SHOW_WINDOW_CMD(i32) — hence the cast.
        wp.showCmd == SW_SHOWMAXIMIZED.0 as u32
    } else {
        // GetWindowPlacement can fail against UIPI-protected (elevated)
        // windows. IsZoomed still answers there.
        IsZoomed(hwnd).as_bool()
    };

    if maximized && have_placement {
        // rcNormalPosition is in WORKSPACE coordinates for any top-level
        // window without WS_EX_TOOLWINDOW — offset from screen coordinates by
        // the work area's inset within its own monitor. That inset is NOT zero
        // on this machine: probed 2026-08-26 via EnumDisplayMonitors, the
        // primary reports rcMonitor (0,0)-(2560,1600) and rcWork
        // (0,48)-(2560,1600) — 48 physical px eaten by a bar docked across the
        // TOP edge (the panel is 2560x1600 at 150%, so 32 logical px). So
        // skipping the conversion is a live 48 px error, not a theoretical
        // one. `restore_window` feeds these bounds to `SetWindowPos`, which
        // wants SCREEN coordinates — convert.
        let sr = normal_position_to_screen(wp.rcNormalPosition, hwnd);
        return Some((
            sr.left,
            sr.top,
            sr.right - sr.left,
            sr.bottom - sr.top,
            maximized,
        ));
    }

    // Normal show state (or the placement read failed): use GetWindowRect.
    // Two reasons this is NOT rcNormalPosition: it is already in screen
    // coordinates, and rcNormalPosition is documented-stale for Aero-Snapped
    // windows (Chromium carries a workaround for exactly this — crbug 36421),
    // so the live rect is the truthful one whenever the window is actually
    // displaying it.
    let mut rect = RECT::default();
    if GetWindowRect(hwnd, &mut rect).is_err() {
        log::warn!("pip: GetWindowRect failed for hwnd {hwnd_key:#x} — not entering PiP");
        return None;
    }
    if maximized {
        // Placement unreadable AND zoomed: the maximised rect is the only
        // measurement available. The 5th tap will re-maximise anyway
        // (was_maximized), so the wrong bounds only matter to a later manual
        // un-maximise. Say so.
        log::warn!(
            "pip: GetWindowPlacement failed for maximised hwnd {hwnd_key:#x} (elevated?) — \
             falling back to its maximised rect as the original"
        );
    }
    Some((
        rect.left,
        rect.top,
        rect.right - rect.left,
        rect.bottom - rect.top,
        maximized,
    ))
}

/// True when this HWND belongs to the Spaceadom process.
#[cfg(windows)]
unsafe fn is_own_window(hwnd: windows::Win32::Foundation::HWND) -> bool {
    use windows::Win32::System::Threading::GetCurrentProcessId;
    use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;

    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    pid != 0 && pid == GetCurrentProcessId()
}

/// Forget every entry whose window no longer exists.
///
/// §10: one `retain` over the ONE map, so BOTH keys' namespaces are pruned by
/// construction. That is the whole argument for putting the mode in the key
/// rather than in a second map — a second container would need a second
/// `retain`, and the day someone forgets it a dead HWND sits in the surviving
/// map waiting to be inherited by a recycled handle (NATIVE_SAFETY rule 3).
#[cfg(windows)]
fn prune_dead(map: &mut HashMap<PipKey, PipEntry>) {
    use windows::Win32::UI::WindowsAndMessaging::IsWindow;
    map.retain(|k, _| {
        let alive =
            unsafe { IsWindow(windows::Win32::Foundation::HWND(k.hwnd as *mut _)).as_bool() };
        if !alive {
            log::debug!(
                "pip: dropping the {} cache entry for closed window {:#x}",
                k.mode.key_name(),
                k.hwnd
            );
        }
        alive
    });
}

/// §10 (PROBLEM 220) — THE TAKEOVER RULE, stated once, here.
///
/// **A window may be held by only ONE of the two PiP keys at a time.** When a
/// tap in mode M lands on a window the OTHER key is already holding, the other
/// key's entry is taken out of the map and handed back so the caller can
/// RESTORE the window first — putting it back exactly the way that feature
/// found it — and only then enter PiP in mode M from a clean window.
///
/// The owner's decision, and the reason for it: two live entries for one HWND
/// is precisely what produced *"it did behave oddly"*. With both entries live,
/// the two features disagree about the window's original bounds, its corner
/// index, its `fullscreen_pip` flag and its captured fullscreen state, and the
/// watcher would release one of them out from under the other. Restoring the
/// first is what makes the second entry's `original_*` an honest measurement of
/// the user's real window instead of a measurement of the other feature's
/// corner tile — the same data-loss rule §8 is built on.
///
/// Pure: it does the map bookkeeping and touches no window. The restore is the
/// caller's, with the lock released, because `restore_window` is
/// SendMessage-class against a foreign app (TASK 4).
#[cfg(windows)]
fn takeover_victim(
    map: &mut HashMap<PipKey, PipEntry>,
    hwnd: isize,
    mode: PipMode,
) -> Option<PipEntry> {
    map.remove(&PipKey {
        hwnd,
        mode: mode.other(),
    })
}

/// Does this window look like one the OLD PiP stripped and abandoned?
///
/// Deliberately narrow. A plain "borderless and topmost" test would also match
/// games, media players in fullscreen and every app with custom chrome, and
/// handing WS_CAPTION to a Chromium window that never had one would wreck its
/// layout. So the signature has to include the geometry the old PiP produced
/// and nothing else does by coincidence: half the work area on BOTH axes,
/// parked within a few pixels of one of the four corners.
#[cfg(windows)]
unsafe fn looks_orphaned(hwnd: windows::Win32::Foundation::HWND) -> bool {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongW, GetWindowRect, GWL_EXSTYLE, GWL_STYLE, WS_CAPTION, WS_EX_TOPMOST,
        WS_THICKFRAME,
    };

    let style = GetWindowLongW(hwnd, GWL_STYLE) as u32;
    let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;

    // Must be topmost AND missing both bits the old code removed.
    if ex_style & WS_EX_TOPMOST.0 == 0 {
        return false;
    }
    if style & WS_CAPTION.0 != 0 || style & WS_THICKFRAME.0 != 0 {
        return false;
    }

    let mut rect = RECT::default();
    if GetWindowRect(hwnd, &mut rect).is_err() {
        return false;
    }
    let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
    let mut mi = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if !GetMonitorInfoW(monitor, &mut mi).as_bool() {
        return false;
    }
    let wa = mi.rcWork;
    let mon_w = wa.right - wa.left;
    let mon_h = wa.bottom - wa.top;
    let (pip_w, pip_h) = (mon_w / 2, mon_h / 2);

    // 4px of slack: integer halving and DPI rounding move things by a pixel or
    // two, but nothing legitimate lands this close to the signature by chance.
    const SLACK: i32 = 4;
    let near = |a: i32, b: i32| (a - b).abs() <= SLACK;

    let w = rect.right - rect.left;
    let h = rect.bottom - rect.top;
    if !near(w, pip_w) || !near(h, pip_h) {
        return false;
    }

    let corners = [
        (wa.left, wa.top),
        (wa.left + mon_w - pip_w, wa.top),
        (wa.left + mon_w - pip_w, wa.top + mon_h - pip_h),
        (wa.left, wa.top + mon_h - pip_h),
    ];
    corners
        .iter()
        .any(|(cx, cy)| near(rect.left, *cx) && near(rect.top, *cy))
}

/// Give a window that the OLD PiP stripped its frame back.
///
/// We cannot know its original size — that died with the build that took it —
/// so this repairs what is actually broken (no title bar to drag, no edge to
/// resize, stuck above everything) and leaves it where it sits. The user can
/// then move and size it normally, which is precisely what they could not do.
#[cfg(windows)]
unsafe fn rescue_orphan(hwnd: windows::Win32::Foundation::HWND) {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongW, SetWindowLongW, SetWindowPos, GWL_STYLE, HWND_NOTOPMOST, SWP_FRAMECHANGED,
        SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, WS_CAPTION, WS_THICKFRAME,
    };

    log::warn!(
        "pip: hwnd {:#x} carries the signature of a window an OLDER build stripped and \
         never restored (borderless + topmost + exactly a quarter-screen corner tile). \
         Giving its title bar and resize edge back and clearing stay-on-top. This path \
         is legacy repair only — this build never strips a window's frame.",
        hwnd.0 as isize
    );

    let style = GetWindowLongW(hwnd, GWL_STYLE);
    SetWindowLongW(
        hwnd,
        GWL_STYLE,
        style | WS_CAPTION.0 as i32 | WS_THICKFRAME.0 as i32,
    );
    // SWP_FRAMECHANGED IS required here, unlike the PiP path: the non-client
    // area genuinely changed size, and without it the window keeps drawing
    // with its old frame metrics until something else forces a recalculation.
    let _ = SetWindowPos(
        hwnd,
        HWND_NOTOPMOST,
        0,
        0,
        0,
        0,
        SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
    );
}

/// Put every window PiP is still holding back the way it was found.
///
/// Called from the Tauri exit handler. Without it, quitting Spaceadom left
/// every PiP'd window pinned above everything else at a quarter size, with the
/// only control that could release them gone — the owner's "loses its title
/// bar and won't come back". Best-effort by design: shutdown must not be able
/// to fail, so every step is ignore-on-error and the map is cleared regardless.
///
/// **RELEASED entries are restored too, and that is the point of keeping them**
/// (§8). Such a window is no longer topmost, but nothing on the machine except
/// this entry still knows its pre-PiP frame — Windows' own `rcNormalPosition`
/// holds the corner tile PiP left there. Filtering them out here would turn
/// "we kept the bounds so they could not be lost" into a slower way of losing
/// them. `restore_window` clearing HWND_NOTOPMOST a second time is a harmless
/// no-op.
pub fn restore_all() {
    #[cfg(windows)]
    {
        // Drain under a short lock, act with the lock RELEASED (Task 4):
        // `restore_window` makes ShowWindow-class calls that can block on a
        // wedged target app, and holding the cache lock across them would
        // stall the fullscreen watcher's release pass for the duration.
        let entries: Vec<(PipKey, PipEntry)> = {
            let mut map = global_cache().lock().unwrap_or_else(|p| p.into_inner());
            if map.is_empty() {
                return;
            }
            let released = map
                .values()
                .filter(|e| e.state == PipState::Released)
                .count();
            log::info!(
                "pip: restoring {} window(s) before exit ({released} of them already \
                 half-released — still restored, because this entry is the only surviving \
                 copy of their pre-PiP bounds)",
                map.len()
            );
            map.drain().collect()
        };
        for (key, entry) in entries {
            let hwnd = windows::Win32::Foundation::HWND(key.hwnd as *mut _);
            unsafe {
                use windows::Win32::UI::WindowsAndMessaging::IsWindow;
                if IsWindow(hwnd).as_bool() {
                    restore_window(hwnd, &entry);
                }
            }
        }
    }
}

#[cfg(windows)]
fn corner_position(
    idx: u8,
    left: i32,
    top: i32,
    pip_w: i32,
    pip_h: i32,
    mon_w: i32,
    mon_h: i32,
) -> (i32, i32) {
    match idx {
        1 => (left + mon_w - pip_w, top),                 // Top-Right
        2 => (left + mon_w - pip_w, top + mon_h - pip_h), // Bottom-Right
        3 => (left, top + mon_h - pip_h),                 // Bottom-Left
        _ => (left, top),                                 // Top-Left (fallback)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// §9 — fullscreen-preserving PiP (Space+Tab). See the header for the
// measurement every line of this section rests on.
// ─────────────────────────────────────────────────────────────────────────────

/// How long after a fullscreen-preserving placement to check that it held.
///
/// MEASURED, not guessed (2026-08-29): a plain `SetWindowPos` on a fullscreen
/// Brave window is reverted to the monitor rect **within 40 ms** — the sample
/// taken 40 ms after the call already showed the window back at full size.
/// 200 ms is five times that, which is enough slack for a busier machine
/// while still being far below the ~500 ms at which a user would read the
/// fallback as a second, separate jump rather than a correction.
#[cfg(windows)]
const FS_VERIFY_MS: u64 = 200;

/// Pixels of slack when comparing a window rect to a target or to a monitor.
///
/// NOT cosmetic. The measurement caught a fullscreen Brave reporting
/// 2560x1599 on a 2560x1600 monitor — one pixel short, and an exact-equality
/// fullscreen test would have called that window "not fullscreen" and sent
/// the owner down the fallback path on the exact case the feature is for.
/// Same value as `looks_orphaned`'s SLACK, for the same DPI-rounding reason.
#[cfg(windows)]
const FS_SLACK: i32 = 4;

/// Pure (§9): is this window geometrically and structurally FULLSCREEN?
///
/// `(l, t, r, b)` for both rects; `mon` is rcMonitor, NOT the work area — a
/// fullscreen window covers the taskbar, which is most of what distinguishes
/// it from a maximised one.
///
/// The three tests, and what each one is for. All three came off the
/// 2026-08-29 measurement, where a genuinely fullscreen Brave reported style
/// `0x160B0000` covering (0,0)-(2560,1600) with `IsZoomed` false, and the
/// ordinary windowed control reported `0x16CF0000` — the two styles differ in
/// exactly `WS_CAPTION | WS_THICKFRAME`.
///
///  · NOT ZOOMED. A maximised window also has no visible border and can sit
///    flush against the work area, but it is not fullscreen: it keeps its
///    caption, the taskbar still shows, and it is `IsZoomed`. Space+` already
///    handles maximised windows correctly and must keep them.
///  · NO CAPTION AND NO THICK FRAME. This is what "the app is drawing no
///    chrome" looks like from outside the process, and it is the half of the
///    test that a merely-large window cannot fake.
///  · COVERS THE MONITOR. The geometric half. Together with the style test it
///    is specific enough that no ordinary window reaches it by accident.
///
/// Fails toward NOT fullscreen on degenerate input, because the consequence
/// of a wrong "yes" is placing suppressed-veto moves on a window that never
/// asked for them, while the consequence of a wrong "no" is the fallback —
/// today's Space+` behaviour, which is always available and never harmful.
#[cfg(windows)]
fn is_fullscreen_geometry(
    style: u32,
    zoomed: bool,
    win: (i32, i32, i32, i32),
    mon: (i32, i32, i32, i32),
) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{WS_CAPTION, WS_THICKFRAME};

    if zoomed {
        return false;
    }
    if style & WS_CAPTION.0 != 0 || style & WS_THICKFRAME.0 != 0 {
        return false;
    }
    let (wl, wt, wr, wb) = win;
    let (ml, mt, mr, mb) = mon;
    // A degenerate monitor rect would make "covers it" trivially true.
    if mr <= ml || mb <= mt {
        return false;
    }
    wl <= ml + FS_SLACK && wt <= mt + FS_SLACK && wr >= mr - FS_SLACK && wb >= mb - FS_SLACK
}

/// Pure (§9): did a placement HOLD, or did the window snap back?
///
/// The verification that turns §9's measurement into a guarantee rather than
/// an assumption about one Chromium build. Compares position and size, both
/// with `FS_SLACK`: a window is free to round a size by a pixel, and is not
/// free to be back at full-monitor size.
#[cfg(windows)]
fn placement_held(target: (i32, i32, i32, i32), actual: (i32, i32, i32, i32)) -> bool {
    let near = |a: i32, b: i32| (a - b).abs() <= FS_SLACK;
    near(target.0, actual.0)
        && near(target.1, actual.1)
        && near(target.2, actual.2)
        && near(target.3, actual.3)
}

/// The `SetWindowPos` flags for a placement on this kind of entry.
///
/// `SWP_NOSENDCHANGING` is the entire mechanism of §9 and the only difference
/// between a fullscreen tile that holds and one that is back at full size
/// 40 ms later. It suppresses the `WM_WINDOWPOSCHANGING` a window receives
/// before a move — which is where Chromium reasserts its monitor bounds while
/// fullscreen — so the veto is never asked for rather than being overruled.
///
/// It is NOT added to the corner path. Space+` is unchanged, and a normal
/// window is entitled to enforce its own minimum size on a resize.
#[cfg(windows)]
fn move_flags(
    fullscreen_pip: bool,
) -> windows::Win32::UI::WindowsAndMessaging::SET_WINDOW_POS_FLAGS {
    use windows::Win32::UI::WindowsAndMessaging::{
        SWP_NOACTIVATE, SWP_NOSENDCHANGING, SWP_NOZORDER,
    };
    if fullscreen_pip {
        SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOSENDCHANGING
    } else {
        SWP_NOZORDER | SWP_NOACTIVATE
    }
}

/// NATIVE_SAFETY §1 rows 1 and 2, as one answer: a named reason this window
/// must not be moved, or `None`.
///
/// Rule 1 of NATIVE_SAFETY is POSITIVE FILTERS, NOT DENYLISTS — a denylist of
/// shell classes was tried and was immediately incomplete. So for explorer.exe
/// the only acceptable class is `CabinetWClass`, a real File Explorer file
/// window; everything else explorer owns is the taskbar, the desktop, the
/// gesture overlays or a helper, and acting on one of those is what broke the
/// owner's touchpad on 2026-08-10.
#[cfg(windows)]
unsafe fn shell_safety_refusal(
    hwnd: windows::Win32::Foundation::HWND,
) -> Option<&'static str> {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetClassNameW, GetWindowTextLengthW, GetWindowThreadProcessId,
    };

    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    let stem = crate::hook::exclusions::process_stem_for_pid(pid);

    if stem == "explorer" {
        let mut cls_buf = [0u16; 64];
        let n = GetClassNameW(hwnd, &mut cls_buf);
        let cls = String::from_utf16_lossy(&cls_buf[..n.max(0) as usize]);
        if cls != "CabinetWClass" {
            return Some("it belongs to explorer.exe and is not a File Explorer window");
        }
        return None;
    }

    // Every other process: a real application main window has a title.
    // Untitled top-levels are IME hosts, tray helpers and splash leftovers.
    if GetWindowTextLengthW(hwnd) == 0 {
        return Some("it has no window title, so it is a helper window, not an app window");
    }
    None
}

/// Read the window and answer `is_fullscreen_geometry` for it — returning, on
/// a yes, the exact state that makes it fullscreen.
///
/// It used to return a bare `bool`. The state is what the RESTORE leg needs
/// (2026-08-29): `original_*` cannot be trusted to be the monitor rect, and
/// `was_maximized` cannot be trusted to be false, so the 5th tap replays this
/// observation instead of re-deriving fullscreen from bounds and flags. The
/// probe is the one place the window is known to be fullscreen, so it is the
/// only honest place to take the copy.
///
/// Bookkeeping-class throughout: `GetWindowLongW`, `GetWindowRect`,
/// `MonitorFromWindow`, `GetMonitorInfoW` and `IsZoomed` all read the window
/// manager's own tables and never dispatch into the target's message pump, so
/// this is safe to run on the engine thread — unlike the `ShowWindow` §6 had
/// to move off it.
#[cfg(windows)]
unsafe fn fullscreen_probe(
    hwnd: windows::Win32::Foundation::HWND,
) -> Option<FullscreenState> {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongW, GetWindowRect, IsZoomed, GWL_EXSTYLE, GWL_STYLE,
    };

    let mut rect = RECT::default();
    if GetWindowRect(hwnd, &mut rect).is_err() {
        log::warn!(
            "fs-pip: GetWindowRect failed for hwnd {:#x} — cannot tell whether it is \
             fullscreen, so it is treated as NOT fullscreen (the fallback is always safe)",
            hwnd.0 as isize
        );
        return None;
    }
    let mut mi = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    // The window's OWN monitor, not the cursor's: "does it cover its screen"
    // is a question about the screen it is actually on. (Corner placement
    // still uses the cursor's monitor — §4, the owner's decision, unchanged.)
    if !GetMonitorInfoW(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST), &mut mi).as_bool() {
        log::warn!(
            "fs-pip: GetMonitorInfoW failed for hwnd {:#x} — treating it as NOT fullscreen",
            hwnd.0 as isize
        );
        return None;
    }
    let style = GetWindowLongW(hwnd, GWL_STYLE) as u32;
    let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
    let zoomed = IsZoomed(hwnd).as_bool();
    let m = mi.rcMonitor;
    let verdict = is_fullscreen_geometry(
        style,
        zoomed,
        (rect.left, rect.top, rect.right, rect.bottom),
        (m.left, m.top, m.right, m.bottom),
    );
    log::info!(
        "fs-pip: hwnd {:#x} fullscreen probe = {verdict} (style {style:#010x}, zoomed={zoomed}, \
         window ({},{})-({},{}) vs monitor ({},{})-({},{}))",
        hwnd.0 as isize,
        rect.left,
        rect.top,
        rect.right,
        rect.bottom,
        m.left,
        m.top,
        m.right,
        m.bottom
    );
    // Captured ONLY on a yes. A `no` has no fullscreen state worth recording,
    // and inventing one would hand `restore_plan` a rect to replay that never
    // described a fullscreen window.
    verdict.then_some(FullscreenState {
        style,
        ex_style,
        x: rect.left,
        y: rect.top,
        w: rect.right - rect.left,
        h: rect.bottom - rect.top,
    })
}

/// §9 — place a FULLSCREEN window into a corner tile without letting it leave
/// fullscreen, then VERIFY it stayed there and fall back if it did not.
///
/// NO SPRING, deliberately, and this is the one place the two PiP paths look
/// different to the eye. `animate_to` issues up to 120 `SetWindowPos` calls
/// over ~960 ms; on a window that is playing video fullscreen each of those
/// is a resize of a live compositing surface, and — more to the point —
/// `animate_to` opens by un-maximising, which is the single call most certain
/// to drag a window out of the state this whole feature exists to preserve.
/// One placement, then one measurement of whether it held.
///
/// Runs on a disposable thread for the usual reason: `SetWindowPos` against a
/// foreign window is SendMessage-class and does not return until that app's
/// WndProc has run it, and this is called from the engine actor (PROBLEM 58 /
/// §6). The verify sleep alone would be reason enough.
#[cfg(windows)]
fn place_fullscreen_preserving(
    hwnd: windows::Win32::Foundation::HWND,
    tx: i32,
    ty: i32,
    tw: i32,
    th: i32,
    cache: PipCache,
    key: PipKey,
    serial: u64,
) {
    use windows::Win32::UI::WindowsAndMessaging::{GetWindowRect, IsWindow, IsZoomed, SetWindowPos};

    let hwnd_raw = hwnd.0 as isize;
    // Retire any spring still in the air for this window — it would keep
    // driving the window with UNSUPPRESSED flags and fight this placement.
    let ticket = ANIM_GEN.fetch_add(1, Ordering::SeqCst) + 1;
    let cancel_token = ANIM_CANCEL.load(Ordering::SeqCst);

    let body = move || {
        let h = || windows::Win32::Foundation::HWND(hwnd_raw as *mut _);

        // A restore or a release means the window is not ours to move at all.
        if ANIM_CANCEL.load(Ordering::SeqCst) != cancel_token {
            return;
        }

        // The sticky-flag edge (see `PipEntry::fullscreen_pip`): a preserved
        // fullscreen entry can be re-entered onto a window that has since
        // been MAXIMISED. A maximised window is never fullscreen — the
        // measurement is unambiguous, fullscreen reports `IsZoomed` false —
        // so moving it with suppressed-veto flags would produce §6's "corner
        // tile that is secretly still a zoomed window". Hand it to the
        // ordinary path, which un-maximises first because that is its job.
        if unsafe { IsZoomed(h()).as_bool() } {
            log::info!(
                "fs-pip: hwnd {hwnd_raw:#x} is MAXIMISED, not fullscreen — there is no \
                 fullscreen state to preserve here. Clearing the flag and using the ordinary \
                 corner path, which un-maximises first."
            );
            disarm_click_guard("the window is maximised, not fullscreen");
            clear_fullscreen_flag(&cache, key, serial);
            animate_to(h(), tx, ty, tw, th);
            return;
        }

        // §10 — ARM THE CLICK GUARD BEFORE THE PLACEMENT, not after the
        // verification. Measured 2026-08-29: a fullscreen Chromium window
        // reasserts its monitor bounds ~16 ms after ANY click into it, and the
        // verification below does not run for FS_VERIFY_MS (200 ms). Arming
        // afterwards would leave a 200 ms hole in which a user who taps
        // Space+Tab and immediately clicks the video gets the snap-back read as
        // "this window refused the tile" and is dropped to corner PiP — losing
        // the fullscreen the feature exists to keep. Armed first, the guard
        // corrects that click inside ~16 ms and the verification then measures
        // a tile that is genuinely holding.
        arm_click_guard(hwnd_raw, tx, ty, tw, th);

        unsafe {
            let _ = SetWindowPos(
                h(),
                windows::Win32::Foundation::HWND(std::ptr::null_mut()),
                tx,
                ty,
                tw,
                th,
                move_flags(true),
            );
        }

        std::thread::sleep(std::time::Duration::from_millis(FS_VERIFY_MS));

        // Superseded or cancelled while we slept: whatever the window looks
        // like now is not ours to judge, and certainly not ours to "correct".
        if ANIM_CANCEL.load(Ordering::SeqCst) != cancel_token
            || ANIM_GEN.load(Ordering::SeqCst) != ticket
        {
            return;
        }
        if !unsafe { IsWindow(h()).as_bool() } {
            return;
        }

        let mut rect = windows::Win32::Foundation::RECT::default();
        if unsafe { GetWindowRect(h(), &mut rect) }.is_err() {
            // Cannot verify. Do NOT fall back on an unreadable window: the
            // fallback moves the window, and moving a window on the strength
            // of a failed measurement is how a working tile gets destroyed.
            // Say so and leave it — the tile is almost certainly fine, and if
            // it is not the 5th tap still restores.
            log::warn!(
                "fs-pip: GetWindowRect failed for hwnd {hwnd_raw:#x} after placement — cannot \
                 verify the tile held. Leaving it as placed rather than moving a window on the \
                 strength of a failed measurement."
            );
            return;
        }
        let actual = (
            rect.left,
            rect.top,
            rect.right - rect.left,
            rect.bottom - rect.top,
        );
        let still_fullscreen = unsafe { fullscreen_probe_style_only(h()) };

        if placement_held((tx, ty, tw, th), actual) && still_fullscreen {
            log::info!(
                "fs-pip: hwnd {hwnd_raw:#x} held its {tw}x{th} tile at ({tx},{ty}) {FS_VERIFY_MS}ms \
                 after placement and is still chrome-less — the window is cornered WITHOUT \
                 having left fullscreen, which is the whole feature."
            );
            return;
        }

        // ── THE FALLBACK (trap 1) ────────────────────────────────────────
        // The app fought the move, or left fullscreen because of it. Either
        // way the fullscreen-preserving premise is dead for this window, and
        // the owner's rule is absolute: never leave the user with a window
        // that is neither fullscreen nor cornered. So this becomes an
        // ordinary corner PiP — flag cleared so that cycling, the watcher and
        // the 5th tap all treat it as one — and the user is told why rather
        // than left to wonder whether the key works.
        log::warn!(
            "fs-pip: hwnd {hwnd_raw:#x} did NOT hold the fullscreen-preserving placement \
             (wanted {tw}x{th} at ({tx},{ty}), found {}x{} at ({},{}); still chrome-less = \
             {still_fullscreen}). The app reasserted its own bounds or dropped out of \
             fullscreen. Falling back to the ordinary corner-snap and clearing the \
             fullscreen flag on the cache entry so every later tap agrees with reality.",
            actual.2,
            actual.3,
            actual.0,
            actual.1
        );
        disarm_click_guard("the window would not hold the fullscreen-preserving tile");
        clear_fullscreen_flag(&cache, key, serial);
        animate_to(h(), tx, ty, tw, th);
        if let Some(handle) = crate::guide_hud::app_handle() {
            crate::show_toast(&handle, "📐 Window left fullscreen — corner PiP");
        }
    };

    // PROBLEM 124 — a spawn failure must degrade, never disappear. Inline is
    // strictly worse (it blocks the engine actor for FS_VERIFY_MS) and still
    // strictly better than no placement at all.
    if let Err(e) = std::thread::Builder::new()
        .name("st-pip-fspip".into())
        .spawn(body)
    {
        log::warn!(
            "fs-pip: could not spawn the placement thread ({e}) — the window is being placed \
             inline on the engine thread instead, which will stall it for ~{FS_VERIFY_MS}ms"
        );
        // Rebuild the closure's work without the thread: the closure was
        // consumed by the failed spawn, so this is the same call sequence
        // reduced to its irreducible half — place it, and skip the verify.
        unsafe {
            let _ = SetWindowPos(
                hwnd,
                windows::Win32::Foundation::HWND(std::ptr::null_mut()),
                tx,
                ty,
                tw,
                th,
                move_flags(true),
            );
        }
    }
}

/// Is the window still drawing no chrome? The style half of the fullscreen
/// test, on its own.
///
/// Used by the verification, where the geometry half would be circular: the
/// window has just been shrunk to a corner tile ON PURPOSE, so "does it cover
/// its monitor" now answers no for the successful case. What still has to be
/// true is that the app did not respond by putting its caption and resize
/// frame back — i.e. by leaving fullscreen.
#[cfg(windows)]
unsafe fn fullscreen_probe_style_only(hwnd: windows::Win32::Foundation::HWND) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongW, GWL_STYLE, WS_CAPTION, WS_THICKFRAME,
    };
    let style = GetWindowLongW(hwnd, GWL_STYLE) as u32;
    style & WS_CAPTION.0 == 0 && style & WS_THICKFRAME.0 == 0
}

// ─────────────────────────────────────────────────────────────────────────────
// §10 — THE CLICK GUARD (PROBLEM 219, amendment 3; 2026-08-29).
//
// THE OWNER'S REPORT: *"interacting with the tab pip just made it full screen
// at the first tap."* A cornered video that fills the screen the moment you
// click it cannot be used at all, which is most of the feature gone.
//
// WHAT WAS MEASURED, and it is again the whole design (2026-08-29, a throwaway
// Brave with its own `--user-data-dir`, never the owner's profile — the method
// that found `SWP_NOSENDCHANGING`). A genuinely fullscreen Brave, style
// `0x160B0000`, tiled to 1280x800 at (0,0) with the suppressed veto:
//
//   · TILED WHILE NOT FOREGROUND, then left alone — holds indefinitely.
//     `SWP_NOSENDCHANGING` works exactly as §9 says it does.
//   · CLICKED — the window is back at (0,0) 2560x1600 at the FIRST sample, and
//     a finer run timed the drift at **+16 ms**. Style stays `0x160B0000` and
//     `IsZoomed` stays false throughout: the window never left fullscreen, it
//     re-ran its fullscreen layout and reasserted the monitor rect. The owner's
//     bug, reproduced.
//   · RE-ASSERTED ONCE, then left alone — holds for as long as it is watched.
//     So being foreground is NOT what breaks it.
//   · CLICKED AGAIN WHILE ALREADY FOREGROUND — snaps back AGAIN. **This is the
//     result that chose the mechanism.** `EVENT_SYSTEM_FOREGROUND` would have
//     fixed the first click and nothing after it, i.e. it would have fixed the
//     sentence the owner wrote ("at the first tap") and not the problem he has.
//   · SIX CLICKS with a re-assert on each observed drift — exactly six
//     corrections, window settled on the tile, no fight.
//   · The same measurements against a real `<video>` in ELEMENT fullscreen (the
//     owner's actual case, not F11) — identical in every respect.
//
// THE SIGNAL. `EVENT_OBJECT_LOCATIONCHANGE` via `SetWinEventHook`, scoped to
// the target window's process. Measured with the hook installed:
//
//   · It DOES fire for Chromium's own snap-back (+31 ms in the observe-only
//     run) — the signal exists, which was the one thing that could have made
//     this approach impossible.
//   · With the callback re-asserting: 7 clicks → 7 corrections, each landing
//     15-31 ms after the drift, and every drift event followed by exactly one
//     non-drift event (our own placement). It CONVERGES; Chromium does not
//     fight back.
//   · Idle 4 s with the guard armed: **0 events, 0 corrections.** The cost when
//     nothing happens is nothing.
//   · A corner cycle with the guard retargeted: 0 spurious corrections. The
//     guard does not fight PiP's own moves.
//
// WHY THE 500 ms WATCHER WAS NOT USED, having been offered. The drift is 16 ms.
// A watcher that ticks every 500 ms would leave the window filling the screen
// for up to half a second on every click — visibly broken, just less often.
//
// WHY THIS DOES NOT FIGHT THE USER. It is armed ONLY for `fullscreen_pip`
// entries, and a window in that state has style `0x160B0000` — no `WS_CAPTION`,
// no `WS_THICKFRAME` (measured, unchanged across every run above). There is no
// title bar to drag it by and no edge to resize it from, so there is no
// user gesture for the guard to overrule. Space+`'s tiles are ordinary windows
// the user CAN drag, and they are never guarded.
//
// PROBLEM 58. Nothing here runs on the keyboard hook's callback. This is its
// own thread with its own message pump, and the WinEvent callback itself is
// atomics plus, only once a drift is real, one `GetWindowRect` and one
// `SetWindowPos`.
// ─────────────────────────────────────────────────────────────────────────────

/// The window the click guard is currently protecting, or 0.
///
/// ONE SLOT, like `UNMAX_IN_FLIGHT` and for the same reason: the cache normally
/// holds 0-1 fullscreen entries. A second fullscreen PiP re-points the guard at
/// the new window and the older one simply goes back to snapping on click —
/// today's behaviour, not a new failure. Documented rather than engineered
/// around, because the two-fullscreen-PiPs-at-once case is not one the owner
/// has.
#[cfg(windows)]
static GUARD_HWND: AtomicIsize = AtomicIsize::new(0);
/// The owning process of `GUARD_HWND`, so the hook can be scoped to it instead
/// of to the whole desktop.
#[cfg(windows)]
static GUARD_PID: AtomicU64 = AtomicU64::new(0);
/// The tile the guarded window must occupy, in screen coordinates. Updated by
/// every corner cycle BEFORE the placement, so the guard never corrects a move
/// PiP itself is making.
#[cfg(windows)]
static GUARD_X: AtomicI32 = AtomicI32::new(0);
#[cfg(windows)]
static GUARD_Y: AtomicI32 = AtomicI32::new(0);
#[cfg(windows)]
static GUARD_W: AtomicI32 = AtomicI32::new(0);
#[cfg(windows)]
static GUARD_H: AtomicI32 = AtomicI32::new(0);
/// Bumped by every arm and every disarm. The guard thread compares it on each
/// timer tick, which is how a disarm reaches a thread parked in `GetMessageW`.
#[cfg(windows)]
static GUARD_EPOCH: AtomicU64 = AtomicU64::new(0);
/// Is the guard thread alive? Prevents a second one being spawned per corner
/// tap — the thread outlives individual placements and re-reads the target.
#[cfg(windows)]
static GUARD_THREAD_RUNNING: AtomicBool = AtomicBool::new(false);
/// Corrections made inside the current runaway window, and when that window
/// opened.
#[cfg(windows)]
static GUARD_CORRECTIONS: AtomicU64 = AtomicU64::new(0);
#[cfg(windows)]
static GUARD_WINDOW_START: AtomicU64 = AtomicU64::new(0);
/// Set by the callback when the correction rate says the app is FIGHTING back.
/// Read by the guard thread's timer tick, which does the disarming and the
/// toast — the callback itself never blocks and never touches Tauri.
#[cfg(windows)]
static GUARD_RUNAWAY: AtomicBool = AtomicBool::new(false);

/// How many corrections inside `GUARD_RUNAWAY_MS` count as "this app is
/// fighting us" rather than "the user is clicking".
///
/// Calibration, from the measurement: Brave costs exactly ONE correction per
/// click, and no human clicks 60 times in 3 seconds. An app that genuinely
/// re-asserts in a loop would blow through this in well under a second. So the
/// threshold separates the two cases by more than an order of magnitude, and
/// errs toward keeping the guard — a wrong trip costs the feature, a wrong keep
/// costs one more correction.
#[cfg(windows)]
const GUARD_MAX_CORRECTIONS: u64 = 60;
#[cfg(windows)]
const GUARD_RUNAWAY_MS: u64 = 3000;
/// How often the guard thread wakes to notice a disarm, a dead window or a
/// runaway. Four times a second while a fullscreen PiP is live, never
/// otherwise.
#[cfg(windows)]
const GUARD_TICK_MS: u32 = 250;

/// Point the click guard at `hwnd` and the tile it must hold, starting the
/// guard thread if it is not already running.
///
/// Called from `place_fullscreen_preserving` for BOTH entry and corner cycling,
/// always before the `SetWindowPos` — so the guard's idea of the target is
/// never behind the window's actual destination, which is what would make it
/// drag a cycling window back to the previous corner.
#[cfg(windows)]
fn arm_click_guard(hwnd_raw: isize, x: i32, y: i32, w: i32, h: i32) {
    use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;

    let mut pid = 0u32;
    unsafe {
        GetWindowThreadProcessId(
            windows::Win32::Foundation::HWND(hwnd_raw as *mut _),
            Some(&mut pid),
        )
    };

    // Target rect FIRST, then the hwnd: the callback bails on a hwnd mismatch
    // before it reads the rect, so publishing the hwnd last means it can never
    // observe the new window with the old window's tile.
    GUARD_X.store(x, Ordering::SeqCst);
    GUARD_Y.store(y, Ordering::SeqCst);
    GUARD_W.store(w, Ordering::SeqCst);
    GUARD_H.store(h, Ordering::SeqCst);
    GUARD_PID.store(pid as u64, Ordering::SeqCst);
    GUARD_HWND.store(hwnd_raw, Ordering::SeqCst);
    GUARD_RUNAWAY.store(false, Ordering::SeqCst);
    GUARD_CORRECTIONS.store(0, Ordering::SeqCst);
    GUARD_WINDOW_START.store(crate::hook::tick_count_pub(), Ordering::SeqCst);
    GUARD_EPOCH.fetch_add(1, Ordering::SeqCst);

    if GUARD_THREAD_RUNNING.swap(true, Ordering::SeqCst) {
        return; // already running; it will pick up the new target itself
    }
    if let Err(e) = std::thread::Builder::new()
        .name("st-pip-fsguard".into())
        .spawn(run_click_guard)
    {
        GUARD_THREAD_RUNNING.store(false, Ordering::SeqCst);
        // PROBLEM 124 — degrade, never disappear. Without the guard the tile
        // still holds until the window is clicked, which is exactly the
        // behaviour 1.0.93 shipped; it is not a new failure and the 5th tap
        // still restores.
        log::warn!(
            "fs-pip: could not spawn the click guard thread ({e}) — the fullscreen tile will \
             still snap back to full size when the window is clicked, which is 1.0.93's \
             behaviour. The 5th tap still restores the window."
        );
    }
}

/// Stop guarding. MUST be called before anything deliberately moves the
/// guarded window somewhere other than its tile — above all the 5th tap, which
/// puts the window back to FULLSCREEN: a guard still armed would read that as
/// drift and drag the window straight back into the corner.
#[cfg(windows)]
fn disarm_click_guard(why: &str) {
    if GUARD_HWND.swap(0, Ordering::SeqCst) == 0 {
        return; // not armed — silent, this is called on every ordinary restore
    }
    GUARD_EPOCH.fetch_add(1, Ordering::SeqCst);
    log::info!("fs-pip: click guard disarmed — {why}");
}

/// The WinEvent callback. Atomics, and — only once a drift is real — one
/// `GetWindowRect` and one `SetWindowPos`.
///
/// It re-asserts the tile with the SAME suppressed-veto flags the tile was
/// placed with (`move_flags(true)`), because the window is still fullscreen and
/// a plain placement would be vetoed exactly as §9 measured.
#[cfg(windows)]
unsafe extern "system" fn click_guard_proc(
    _hook: windows::Win32::UI::Accessibility::HWINEVENTHOOK,
    _event: u32,
    hwnd: windows::Win32::Foundation::HWND,
    id_object: i32,
    id_child: i32,
    _thread: u32,
    _time: u32,
) {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::UI::WindowsAndMessaging::{GetWindowRect, SetWindowPos};

    // OBJID_WINDOW / CHILDID_SELF. A LOCATIONCHANGE for a caret, a scrollbar or
    // any other child object is not the window moving, and acting on one would
    // make the guard fire on ordinary page activity.
    if id_object != 0 || id_child != 0 {
        return;
    }
    let target = GUARD_HWND.load(Ordering::SeqCst);
    if target == 0 || hwnd.0 as isize != target {
        return;
    }

    let (x, y, w, h) = (
        GUARD_X.load(Ordering::SeqCst),
        GUARD_Y.load(Ordering::SeqCst),
        GUARD_W.load(Ordering::SeqCst),
        GUARD_H.load(Ordering::SeqCst),
    );
    let mut r = RECT::default();
    if GetWindowRect(hwnd, &mut r).is_err() {
        return;
    }
    let actual = (r.left, r.top, r.right - r.left, r.bottom - r.top);
    // The same slack the entry verification uses: a window is free to round a
    // size by a pixel and is not free to be back at full-monitor size.
    if placement_held((x, y, w, h), actual) {
        return;
    }

    // Runaway detection. An app that re-asserts in a loop would otherwise have
    // us re-assert in a loop with it, burning a core for as long as the PiP
    // lived. The callback only RAISES the flag; the guard thread acts on it,
    // because disarming and toasting from here would mean doing Tauri work on
    // a WinEvent callback.
    let now = crate::hook::tick_count_pub();
    if now.saturating_sub(GUARD_WINDOW_START.load(Ordering::SeqCst)) > GUARD_RUNAWAY_MS {
        GUARD_WINDOW_START.store(now, Ordering::SeqCst);
        GUARD_CORRECTIONS.store(0, Ordering::SeqCst);
    }
    if GUARD_CORRECTIONS.fetch_add(1, Ordering::SeqCst) + 1 > GUARD_MAX_CORRECTIONS {
        GUARD_RUNAWAY.store(true, Ordering::SeqCst);
        return;
    }

    let _ = SetWindowPos(
        hwnd,
        windows::Win32::Foundation::HWND(std::ptr::null_mut()),
        x,
        y,
        w,
        h,
        move_flags(true),
    );
}

/// The guard thread: install the hook for the guarded window's process, pump
/// messages so the out-of-context callback can be delivered, and notice a
/// disarm, a retarget onto a different process, a dead window or a runaway.
#[cfg(windows)]
fn run_click_guard() {
    use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent};
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, GetMessageW, IsWindow, KillTimer, SetTimer, TranslateMessage,
        EVENT_OBJECT_LOCATIONCHANGE, MSG, WINEVENT_OUTOFCONTEXT,
    };

    log::info!(
        "fs-pip: click guard thread started — it re-asserts the corner tile whenever the \
         guarded fullscreen window moves itself off it. Measured 2026-08-29: Chromium \
         reasserts its monitor bounds ~16ms after EVERY click into a fullscreen window, and \
         one re-assert per click is enough (7 clicks, 7 corrections, no fight)."
    );

    // Outer loop: one iteration per PROCESS the guard is pointed at. A hook is
    // scoped to a pid, so retargeting onto a window in a different process
    // means tearing this hook down and installing another.
    loop {
        let hwnd_raw = GUARD_HWND.load(Ordering::SeqCst);
        if hwnd_raw == 0 {
            break;
        }
        let pid = GUARD_PID.load(Ordering::SeqCst) as u32;
        let epoch = GUARD_EPOCH.load(Ordering::SeqCst);

        let hook = unsafe {
            SetWinEventHook(
                EVENT_OBJECT_LOCATIONCHANGE,
                EVENT_OBJECT_LOCATIONCHANGE,
                None,
                Some(click_guard_proc),
                pid,
                0,
                WINEVENT_OUTOFCONTEXT,
            )
        };
        if hook.is_invalid() {
            log::warn!(
                "fs-pip: SetWinEventHook failed for pid {pid} — the fullscreen tile will snap \
                 back to full size when the window is clicked (1.0.93's behaviour). The 5th \
                 tap still restores."
            );
            break;
        }

        // A thread timer (hwnd = None) posts WM_TIMER into THIS thread's queue,
        // which is what lets a thread parked in GetMessageW notice a disarm.
        let timer = unsafe { SetTimer(None, 0, GUARD_TICK_MS, None) };
        let mut msg = MSG::default();
        let mut reinstall = false;
        loop {
            let got = unsafe { GetMessageW(&mut msg, None, 0, 0) };
            // -1 is the documented ERROR return, and `.as_bool()` reads it as
            // true — an unguarded loop here would spin forever on it.
            if got.0 == -1 || got.0 == 0 {
                break;
            }
            unsafe {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }

            let now_hwnd = GUARD_HWND.load(Ordering::SeqCst);
            if now_hwnd == 0 || GUARD_EPOCH.load(Ordering::SeqCst) != epoch {
                // Disarmed, or re-armed onto a new target. Either way this
                // hook's scope may be wrong; drop out and re-decide.
                reinstall = now_hwnd != 0;
                break;
            }
            if !unsafe { IsWindow(windows::Win32::Foundation::HWND(now_hwnd as *mut _)) }.as_bool()
            {
                log::info!(
                    "fs-pip: the guarded window {now_hwnd:#x} is gone — disarming the click \
                     guard. (The cache entry is pruned separately, by `prune_dead` and by the \
                     release watcher.)"
                );
                GUARD_HWND.store(0, Ordering::SeqCst);
                break;
            }
            if GUARD_RUNAWAY.load(Ordering::SeqCst) {
                log::warn!(
                    "fs-pip: the guarded window {now_hwnd:#x} was corrected more than \
                     {GUARD_MAX_CORRECTIONS} times in {GUARD_RUNAWAY_MS}ms — this app is \
                     FIGHTING the tile rather than reasserting once per click (Brave costs \
                     exactly one, measured). Disarming rather than burning a core on it; the \
                     window will snap back to full size on click, which is 1.0.93's \
                     behaviour, and the 5th tap still restores it."
                );
                GUARD_HWND.store(0, Ordering::SeqCst);
                GUARD_RUNAWAY.store(false, Ordering::SeqCst);
                if let Some(handle) = crate::guide_hud::app_handle() {
                    crate::show_toast(&handle, "📺 This app will not hold the corner");
                }
                break;
            }
        }

        unsafe {
            if timer != 0 {
                let _ = KillTimer(None, timer);
            }
            let _ = UnhookWinEvent(hook);
        }
        if !reinstall {
            break;
        }
    }

    GUARD_THREAD_RUNNING.store(false, Ordering::SeqCst);
    log::info!("fs-pip: click guard thread exited");
}

/// Demote a fullscreen entry to an ordinary corner PiP, if it is still the
/// entry we were spawned for.
///
/// The serial match is the same discipline as `claim_for_release`: between
/// the placement and the verify the user can have restored this window and
/// entered PiP again, and clearing the flag on that NEW entry would silently
/// turn a working fullscreen PiP into a corner one.
#[cfg(windows)]
fn clear_fullscreen_flag(cache: &PipCache, key: PipKey, serial: u64) {
    let mut map = cache.lock().unwrap_or_else(|p| p.into_inner());
    match map.get_mut(&key) {
        Some(e) if e.serial == serial => {
            e.fullscreen_pip = false;
            // The CAPTURE goes with the flag. A demoted entry is an ordinary
            // corner PiP, and leaving a fullscreen state hanging off it would
            // let a later re-entry's `.or_else` carry it forward and hand
            // `restore_plan` a fullscreen to replay for a window that provably
            // refused to stay in one.
            e.fullscreen_state = None;
        }
        _ => log::info!(
            "fs-pip: not clearing the fullscreen flag for hwnd {:#x} — the entry under that \
             key is no longer serial {serial}, so a newer PiP owns this window",
            key.hwnd
        ),
    }
}

/// The Space+Tab corner labels. Separate from `corner_label` so the toast
/// says which of the two PiPs the user is looking at — with a fullscreen tile
/// there is no title bar to read the answer off.
#[cfg(windows)]
fn fullscreen_corner_label(idx: u8) -> String {
    match idx {
        0 => "📺 Fullscreen PiP: Top-Left".into(),
        1 => "📺 Fullscreen PiP: Top-Right".into(),
        2 => "📺 Fullscreen PiP: Bottom-Right".into(),
        3 => "📺 Fullscreen PiP: Bottom-Left".into(),
        _ => "📺 Fullscreen PiP".into(),
    }
}

#[cfg(windows)]
fn corner_label(idx: u8) -> String {
    match idx {
        0 => "📐 PiP: Top-Left".into(),
        1 => "📐 PiP: Top-Right".into(),
        2 => "📐 PiP: Bottom-Right".into(),
        3 => "📐 PiP: Bottom-Left".into(),
        _ => "📐 PiP".into(),
    }
}

/// Animate a window to a target rect with a spring, on a short-lived thread so
/// the engine actor is never blocked.
///
/// THE UN-MAXIMISE IS A PRECONDITION, NOT A PARAMETER (2026-08-26, review
/// round 2). It used to be an `unmaximize_first` flag set only on first entry,
/// and that produced a flight whose stale placement could land seconds late:
///
///   T1 (first entry, maximised window) blocks inside the cross-thread
///   `ShowWindow(SW_RESTORE)`. The user taps again to cycle corners — nothing
///   debounces that, cycling is meant to be tapped fast — so T2 spawns with
///   `unmaximize_first = false`, springs to corner 1, finishes and exits.
///   Only THEN does T1's ShowWindow return; T1 finds itself superseded and,
///   under the old "snap to your own target and bow out" rule, placed the
///   window at corner 0. Seconds after the user last touched it, the window
///   silently walked back a corner, and `position_index` no longer described
///   where it was.
///
/// Every flight now checks `IsZoomed` itself. That closes the hole at the
/// source rather than patching the symptom: T2's own `ShowWindow` queues
/// behind T1's in the target's message queue, so T2 *cannot* finish while T1
/// is still blocked — and a flight that finds itself superseded after an
/// unbounded block now returns without placing at all.
#[cfg(windows)]
fn animate_to(hwnd: windows::Win32::Foundation::HWND, tx: i32, ty: i32, tw: i32, th: i32) {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowRect, IsWindow, IsZoomed, SetWindowPos, ShowWindow, SWP_NOACTIVATE, SWP_NOZORDER,
        SW_RESTORE,
    };

    let hwnd_raw = hwnd.0 as isize;
    let ticket = ANIM_GEN.fetch_add(1, Ordering::SeqCst) + 1;
    // Snapshot of the cancel epoch. If restore/release bumps it, this flight
    // stops WITHOUT placing — see ANIM_CANCEL for why that differs from the
    // ticket supersede below.
    let cancel_token = ANIM_CANCEL.load(Ordering::SeqCst);

    let spawned = std::thread::Builder::new()
        .name("st-pip-anim".into())
        .spawn(move || {
            let h = || windows::Win32::Foundation::HWND(hwnd_raw as *mut _);
            // A null insert-after with SWP_NOZORDER: the z-order is untouched,
            // so the topmost flag set when PiP was entered survives the flight.
            let place = |x: i32, y: i32| unsafe {
                let _ = SetWindowPos(
                    h(),
                    windows::Win32::Foundation::HWND(std::ptr::null_mut()),
                    x,
                    y,
                    tw,
                    th,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
            };

            // §6 — the deferred un-maximise, on THIS disposable thread rather
            // than the engine's. A corner tile that is secretly still a zoomed
            // window is not PiP, so any flight that finds one un-maximises it.
            if unsafe { IsZoomed(h()).as_bool() } {
                // Check the cancel epoch BEFORE issuing the call, not only
                // after. `restore_all()` runs synchronously on the Tauri exit
                // thread: it bumps ANIM_CANCEL, moves the window back and
                // re-maximises it if that is how it was found. ANIM_CANCEL
                // cannot preempt a Win32 call already in flight, so a bump
                // that lands in the gap between the snapshot (taken on the
                // engine thread) and this thread actually starting would let
                // an un-maximise fire AFTER shutdown had already re-maximised
                // the window — leaving the user's window in the wrong show
                // state, which is precisely what restore-on-exit promises not
                // to do. Checking here shrinks that window from "however long
                // thread startup takes" to the gap between this load and the
                // syscall below.
                if ANIM_CANCEL.load(Ordering::SeqCst) != cancel_token {
                    return;
                }
                // While this marker names the window, its zoomed-ness belongs
                // to US, not to the user, and the release watcher must not
                // read it as "the user maximised their PiP window". The guard
                // clears it however this scope is left.
                UNMAX_IN_FLIGHT.store(hwnd_raw, Ordering::SeqCst);
                struct UnmaxGuard;
                impl Drop for UnmaxGuard {
                    fn drop(&mut self) {
                        UNMAX_IN_FLIGHT.store(0, Ordering::SeqCst);
                    }
                }
                let _unmax_guard = UnmaxGuard;

                let t = crate::hook::tick_count_pub();
                unsafe {
                    let _ = ShowWindow(h(), SW_RESTORE);
                }
                let took = crate::hook::tick_count_pub().saturating_sub(t);
                log::info!(
                    "pip: deferred un-maximise of hwnd {hwnd_raw:#x} took {took}ms on the \
                     animation thread (the cross-thread relayout that used to block the \
                     engine — if THIS number is large, the target app's pump is the lag)"
                );
                // The world may have moved on while ShowWindow was blocked,
                // by an unbounded amount. A restore/release means hands off
                // entirely. A newer tap means its flight owns the window —
                // and unlike the supersede test inside the spring loop below,
                // we can NOT snap to our own target on the way out: the loop's
                // parting snap is safe because the superseding flight is
                // provably still running and re-drives the window every 8 ms,
                // whereas after an unbounded block that flight may have
                // finished and exited, which would make our "parting" snap the
                // last word — dragging the window back to a corner the user
                // left several taps ago.
                if ANIM_CANCEL.load(Ordering::SeqCst) != cancel_token
                    || ANIM_GEN.load(Ordering::SeqCst) != ticket
                {
                    return;
                }
            }

            // Spring constants (120fps tuned)
            let k: f64 = 0.18;
            let c: f64 = 0.42;

            let mut rect = RECT::default();
            unsafe { GetWindowRect(h(), &mut rect).ok() };

            let mut x = rect.left as f64;
            let mut y = rect.top as f64;
            let mut vx = 0f64;
            let mut vy = 0f64;

            for _ in 0..120 {
                // Cancelled by restore/release: the window is no longer ours
                // to move — stop with NO parting placement (a stale corner
                // snap here is what could silently undo a restore).
                if ANIM_CANCEL.load(Ordering::SeqCst) != cancel_token {
                    return;
                }
                // Superseded by a newer tap: land on OUR target and stand down,
                // rather than fighting the new flight for the same window or
                // abandoning this one part-way to a corner. Safe HERE and not
                // after the un-maximise above, because the superseding flight
                // bumped the counter microseconds ago and is provably still
                // driving the window — this snap is at most one 8 ms frame of
                // disagreement, not the last word.
                if ANIM_GEN.load(Ordering::SeqCst) != ticket {
                    place(tx, ty);
                    return;
                }
                // The window can be closed mid-flight; SetWindowPos on a dead
                // HWND is harmless but pointless, and the loop would keep
                // running for a second for nothing.
                if !unsafe { IsWindow(h()).as_bool() } {
                    return;
                }

                let fx = -k * (x - tx as f64) - c * vx;
                let fy = -k * (y - ty as f64) - c * vy;
                vx += fx;
                vy += fy;
                x += vx;
                y += vy;

                place(x.round() as i32, y.round() as i32);

                // BOTH axes. Testing X alone (what this did until PROBLEM 167)
                // meant a purely vertical hop — Top-Right to Bottom-Right —
                // satisfied the test on iteration 1 and snapped with no motion
                // at all, while the horizontal hops either side of it glided.
                let settled_x = (x - tx as f64).abs() < 0.5 && vx.abs() < 0.5;
                let settled_y = (y - ty as f64).abs() < 0.5 && vy.abs() < 0.5;
                if settled_x && settled_y {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(8));
            }

            // Snap to exact target — the spring gets close, not exact. Guarded
            // by BOTH counters: not superseded, not cancelled.
            if ANIM_CANCEL.load(Ordering::SeqCst) == cancel_token
                && ANIM_GEN.load(Ordering::SeqCst) == ticket
            {
                place(tx, ty);
            }
        });

    // A thread that will not spawn must not lose the window: place it directly.
    // Same lesson as PROBLEM 124 — a spawn failure is plausible on someone
    // else's machine and must degrade to "no animation", never to "no PiP".
    // The un-maximise degrades with it, back to the old blocking-but-correct
    // behaviour: a laggy entry beats a corner tile that is secretly still a
    // maximised window.
    if let Err(e) = spawned {
        log::warn!("pip: could not spawn the animation thread ({e}) — placing the window directly");
        unsafe {
            if IsZoomed(hwnd).as_bool() {
                let _ = ShowWindow(hwnd, SW_RESTORE);
            }
            // That ShowWindow blocks on the target, so re-check the cancel
            // epoch before placing — same reasoning as the threaded path. A
            // release that landed while we were blocked has just written the
            // true pre-PiP bounds into rcNormalPosition, and placing a corner
            // tile on a now-NORMAL window would make Windows overwrite them
            // with the tile. This branch is rare (thread spawn failed), but
            // "rare" is exactly the class of branch PROBLEM 118 was about.
            if ANIM_CANCEL.load(Ordering::SeqCst) != cancel_token {
                return;
            }
            let _ = SetWindowPos(hwnd, windows::Win32::Foundation::HWND(std::ptr::null_mut()), tx, ty, tw, th, SWP_NOZORDER | SWP_NOACTIVATE);
        }
    }
}

/// What the 5th tap (and `restore_all`) must actually do for this entry.
///
/// Pure, and split out for exactly the reason PROBLEM 219's restore leg was
/// wrong: the decision used to be two `if`s inside a Win32 function that no
/// test could reach, so "a fullscreen entry takes the maximize path" was
/// invisible until the owner watched a video come back as a maximised browser.
#[cfg(windows)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RestorePlan {
    /// Today's behaviour, unchanged and still correct for `PipMode::Corner`:
    /// place `original_*`, then re-maximise if the window was maximised.
    Frame {
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        maximize: bool,
    },
    /// §9's restore leg: replay the captured fullscreen state — style,
    /// ex-style and the rect the window covered — with the veto suppressed,
    /// and NEVER the maximize path.
    Fullscreen(FullscreenState),
}

/// Pure (§9 restore leg): which of the two restores does this entry get?
///
/// A fullscreen entry with a capture replays it. EVERYTHING else — every
/// `PipMode::Corner` entry, and a fullscreen entry whose capture is missing —
/// gets today's frame restore, because that is the floor the feature promises
/// never to go below: a window is either put back to fullscreen or put back
/// the way Space+` would have put it back, never left in between.
///
/// Note what is deliberately NOT consulted for a fullscreen entry:
/// `original_*` and `was_maximized`. Both were measured by
/// `measure_original_frame`, and on a Brave window that was maximised before
/// it went fullscreen that measurement describes the PRE-FULLSCREEN WINDOW —
/// `restored frame 2558x1550 at (1,49), maximized=true` in the owner's own
/// log. Placing those back and then re-maximising is what set `WS_MAXIMIZE`
/// (style 0x160B0000 → 0x170B0000, `zoomed=true`) and made the NEXT Space+Tab
/// probe false and fall back to corner PiP.
#[cfg(windows)]
fn restore_plan(entry: &PipEntry) -> RestorePlan {
    match (entry.fullscreen_pip, entry.fullscreen_state) {
        (true, Some(fs)) => RestorePlan::Fullscreen(fs),
        _ => RestorePlan::Frame {
            x: entry.original_x,
            y: entry.original_y,
            w: entry.original_w,
            h: entry.original_h,
            maximize: entry.was_maximized,
        },
    }
}

/// Restore a window to the state, position, size and z-order it had before PiP.
///
/// `HWND_NOTOPMOST` is the part that matters most on every path — leaving a
/// window pinned above everything is what hid the Guide HUD behind it (§3).
///
/// TWO RESTORES, chosen by `restore_plan`. The corner one is byte-for-byte
/// what this function has always done. The fullscreen one exists because
/// PROBLEM 219 shipped with only the corner one, and it left the owner's Brave
/// MAXIMISED instead of fullscreen — see `restore_plan` for the measurement.
#[cfg(windows)]
unsafe fn restore_window(hwnd: windows::Win32::Foundation::HWND, entry: &PipEntry) {
    use windows::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, ShowWindow, HWND_NOTOPMOST, SWP_NOACTIVATE, SW_SHOWMAXIMIZED,
    };

    // Halt any flight still in the air for this window WITHOUT letting it
    // leave a parting corner snap. This used to bump ANIM_GEN, whose
    // supersede semantics make a retired flight place its own target one
    // final time — right for a newer corner tap (that tap's flight
    // immediately takes over), wrong here: the parting snap could land AFTER
    // the SetWindowPos below and quietly drag the just-restored window back
    // to a corner tile, with nothing left in the cache to fix it.
    ANIM_CANCEL.fetch_add(1, Ordering::SeqCst);

    // §10 — AND THE CLICK GUARD, before anything is placed. This restore is
    // about to move the window somewhere that is NOT its tile (for a fullscreen
    // entry: back to the whole monitor). A guard still armed would see that as
    // drift and pull the window straight back into the corner, so the 5th tap
    // would appear to do nothing at all.
    disarm_click_guard("the window is being restored out of PiP");

    let hwnd_key = hwnd.0 as isize;

    // The one case `restore_plan` cannot report by itself: an entry that still
    // claims to be a fullscreen PiP but carries no capture to replay. It should
    // be unreachable — the flag and the capture are set and cleared together —
    // so if it ever happens the user gets today's restore and is TOLD, rather
    // than silently getting something other than the key he pressed.
    if entry.fullscreen_pip && entry.fullscreen_state.is_none() {
        log::warn!(
            "fs-pip: hwnd {hwnd_key:#x} is flagged as a fullscreen PiP but carries no captured \
             fullscreen state, so there is nothing to put back. Falling back to the ordinary \
             frame restore (its pre-PiP bounds, and its maximised state if it had one) — the \
             window ends up exactly where Space+` would have left it, which is the floor this \
             feature promises never to go below."
        );
        if let Some(handle) = crate::guide_hud::app_handle() {
            crate::show_toast(&handle, "↩️ Fullscreen state lost — frame restored");
        }
    }

    match restore_plan(entry) {
        // ── §9's RESTORE LEG (PROBLEM 219, amended 2026-08-29) ────────────
        RestorePlan::Fullscreen(fs) => {
            use windows::Win32::UI::WindowsAndMessaging::{
                GetWindowLongW, IsZoomed, SetWindowLongW, GWL_EXSTYLE, GWL_STYLE,
                SWP_FRAMECHANGED, SWP_NOSENDCHANGING, SW_RESTORE,
            };

            // A fullscreen window is NEVER zoomed — `is_fullscreen_geometry`
            // rejects a zoomed window outright — so if WS_MAXIMIZE is set here
            // something added it after the capture. Clear it the documented
            // way (SW_RESTORE) rather than punching the style bit out: the
            // window manager keeps maximised-ness in more than that one bit.
            // Normally a no-op, because a corner tile is not zoomed.
            if IsZoomed(hwnd).as_bool() {
                log::info!(
                    "fs-pip: hwnd {hwnd_key:#x} is MAXIMISED on the way out of PiP, which no \
                     fullscreen window is — un-maximising first, or WS_MAXIMIZE would survive \
                     into the reinstated fullscreen state and the next probe would refuse it."
                );
                let _ = ShowWindow(hwnd, SW_RESTORE);
            }

            // Put the style back only if it actually moved. A style write needs
            // SWP_FRAMECHANGED to force the non-client recalculation — and
            // asking a Chromium window for one it does NOT need is how you make
            // it flicker (see the entry path's "No SWP_FRAMECHANGED"), so the
            // flag is conditional on a real change.
            let now_style = GetWindowLongW(hwnd, GWL_STYLE) as u32;
            let now_ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
            let mut style_changed = false;
            if now_style != fs.style {
                SetWindowLongW(hwnd, GWL_STYLE, fs.style as i32);
                style_changed = true;
            }
            if now_ex != fs.ex_style {
                SetWindowLongW(hwnd, GWL_EXSTYLE, fs.ex_style as i32);
                style_changed = true;
            }

            // SWP_NOSENDCHANGING on the way back too — MEASURED, not assumed
            // (2026-08-29, a throwaway Brave with its own --user-data-dir, the
            // same method the entry flag was found with): this composition
            // lands the window at the full monitor rect with style back to
            // 0x160B0000 and `IsZoomed` false, and it is still there 2.5 s
            // later. `move_flags` is not reused wholesale because it
            // contributes SWP_NOZORDER, and the z-order change to
            // HWND_NOTOPMOST is the most important thing restore does (§3).
            let mut flags = SWP_NOACTIVATE | SWP_NOSENDCHANGING;
            if style_changed {
                flags |= SWP_FRAMECHANGED;
            }
            let _ = SetWindowPos(hwnd, HWND_NOTOPMOST, fs.x, fs.y, fs.w, fs.h, flags);

            // THE VERIFICATION, logged by the SAME probe the entry uses, so a
            // future failure is one `fs-pip: … fullscreen probe` grep away
            // instead of a fresh diagnostic round trip. This is the line that
            // would have caught the broken restore leg on the night it
            // shipped: the log printed `probe = false (style 0x170B0000,
            // zoomed=true)` only because the NEXT tap happened to ask.
            if fullscreen_probe(hwnd).is_some() {
                log::info!(
                    "fs-pip: hwnd {hwnd_key:#x} restored to TRUE FULLSCREEN — style {:#010x} \
                     reinstated at ({},{}) {}x{}, not maximised. That is the 5th tap working.",
                    fs.style,
                    fs.x,
                    fs.y,
                    fs.w,
                    fs.h
                );
                return;
            }

            // The state would not go back: the app changed its own style while
            // cornered, or the page left fullscreen. The owner's rule is
            // absolute — never leave a window that is neither fullscreen nor
            // properly restored — so finish with today's frame restore, which
            // is where Space+` would have left it, and say why.
            log::warn!(
                "fs-pip: hwnd {hwnd_key:#x} did NOT come back as fullscreen (see the probe line \
                 above) even after its captured style {:#010x} and rect ({},{}) {}x{} were \
                 reinstated — the app changed its own state while it was cornered. Completing \
                 an ordinary frame restore to the pre-PiP bounds instead.",
                fs.style,
                fs.x,
                fs.y,
                fs.w,
                fs.h
            );
            let _ = SetWindowPos(
                hwnd,
                HWND_NOTOPMOST,
                entry.original_x,
                entry.original_y,
                entry.original_w,
                entry.original_h,
                SWP_NOACTIVATE,
            );
            if entry.was_maximized {
                let _ = ShowWindow(hwnd, SW_SHOWMAXIMIZED);
            }
            if let Some(handle) = crate::guide_hud::app_handle() {
                crate::show_toast(&handle, "↩️ Left fullscreen — frame restored");
            }
        }

        // ── Space+`'s restore, byte-for-byte what it has always been ──────
        RestorePlan::Frame { x, y, w, h, maximize } => {
            let _ = SetWindowPos(hwnd, HWND_NOTOPMOST, x, y, w, h, SWP_NOACTIVATE);
            if maximize {
                let _ = ShowWindow(hwnd, SW_SHOWMAXIMIZED);
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// §7 — release always-on-top when the PiP'd window itself is enlarged.
// ─────────────────────────────────────────────────────────────────────────────

/// Release PiP for any cached window that has grown beyond its corner tile
/// (maximised or fullscreened).
///
/// Called from the fullscreen watcher's EXISTING 500 ms tick — the owner's
/// explicit decision: no new thread, no new timer ("THE LAG I ALREADY SAW,
/// WHILE 4 CORNERS, I DONT WANT MORE LAG"). With no PiP active the entire
/// cost is one uncontended mutex lock and an is_empty check.
///
/// Everything this function itself does is bookkeeping-class: map operations
/// and Win32 reads that never dispatch into a foreign message pump. The two
/// calls that DO — `SetWindowPlacement` and the `HWND_NOTOPMOST`
/// `SetWindowPos` — are handed to a short-lived thread, which also raises the
/// toast, because the caller is the sole writer of `hook::FULLSCREEN_ACTIVE`
/// and a watcher blocked inside a wedged app's WndProc strands that flag (see
/// the §7 note in the header, and PROBLEM 88 in `hook/fullscreen.rs`).
pub fn release_enlarged() {
    #[cfg(windows)]
    unsafe {
        release_enlarged_win32()
    }
}

/// One warn every 5 s at most for a probe call that failed. These run twice a
/// second per cached window, so an unthrottled warn would bury the log it is
/// meant to explain; silence is worse still (PROBLEM 169 — "a placement that
/// cannot find a monitor must say so").
#[cfg(windows)]
fn warn_probe_throttled(msg: std::fmt::Arguments<'_>) {
    // 0 doubles as "never logged", so the first failure always speaks.
    static LAST: AtomicU64 = AtomicU64::new(0);
    const EVERY_MS: u64 = 5000;
    let now = crate::hook::tick_count_pub();
    let last = LAST.load(Ordering::Relaxed);
    if last != 0 && now.saturating_sub(last) < EVERY_MS {
        return;
    }
    LAST.store(now.max(1), Ordering::Relaxed);
    log::warn!("{msg}");
}

#[cfg(windows)]
unsafe fn release_enlarged_win32() {
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetClassNameW, GetWindowRect, GetWindowThreadProcessId, IsWindow, IsZoomed,
    };

    let cache = global_cache();

    // Snapshot under a short lock, evaluate with the lock RELEASED (Task 4):
    // this runs on the watcher thread while the engine may be mid-tap, and
    // neither side may ever wait on the other across a Win32 call.
    let snapshot: Vec<(PipKey, PipEntry)> = {
        let map = cache.lock().unwrap_or_else(|p| p.into_inner());
        if map.is_empty() {
            return;
        }
        map.iter().map(|(k, e)| (*k, e.clone())).collect()
    };

    let now = crate::hook::tick_count_pub();

    for (key, entry) in snapshot {
        let hwnd = HWND(key.hwnd as *mut _);
        // `key` is a (hwnd, mode) pair now (§10); the logs still name the
        // window, which is what a reader greps for.
        let khwnd = key.hwnd;

        // Entry too young to judge — on first entry the window can STILL be
        // maximised for a moment (the un-maximise is deferred, §6), and that
        // transient must not read as "the user maximised their PiP window".
        if now.saturating_sub(entry.entered_at) < ENTRY_SETTLE_MS {
            continue;
        }

        // The same transient, but as a FACT instead of a timer: our own
        // deferred un-maximise is currently blocked inside this window's
        // WndProc, so it is still zoomed because of us. A wedged pump can
        // outlast any fixed grace, and releasing here would tear down a PiP
        // the user just asked for.
        if UNMAX_IN_FLIGHT.load(Ordering::SeqCst) == key.hwnd {
            continue;
        }

        if !IsWindow(hwnd).as_bool() {
            // The engine prunes dead entries on the next tap; prune here too
            // so a dead entry cannot sit for hours waiting to be inherited by
            // a recycled HWND between taps.
            cache.lock().unwrap_or_else(|p| p.into_inner()).remove(&key);
            continue;
        }

        // NATIVE_SAFETY rule 3 — never trust a cached HWND. This thread acts
        // on handles cached long ago; if Windows recycled one onto another
        // process's window, releasing "PiP" here would rewrite a stranger's
        // restore bounds — damage that stays invisible until that user
        // un-maximises. A pid mismatch is the tell: drop the entry, touch
        // NOTHING.
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 || pid != entry.pid {
            log::warn!(
                "pip: cached hwnd {khwnd:#x} now belongs to pid {pid} (entry was pid {}) — \
                 the handle was recycled; dropping the entry without touching the window",
                entry.pid
            );
            cache.lock().unwrap_or_else(|p| p.into_inner()).remove(&key);
            continue;
        }

        let zoomed = IsZoomed(hwnd).as_bool();
        let mut rect = RECT::default();
        if GetWindowRect(hwnd, &mut rect).is_err() {
            // Not silent. A window that keeps failing this probe is never
            // released and never explained: it sits HWND_TOPMOST while the
            // watcher retries twice a second with nothing in the log to say
            // why. Every other skip in this loop says something; so does this.
            warn_probe_throttled(format_args!(
                "pip: GetWindowRect failed for cached hwnd {khwnd:#x} — cannot judge whether it \
                 outgrew its corner tile, so it stays topmost and tracked. Retrying every \
                 tick; this warning is throttled to one per 5s."
            ));
            continue;
        }
        // Measured against the window's CURRENT monitor: the user may have
        // dragged the tile to another display since entry, and "grew beyond a
        // corner tile" only means anything relative to the screen it is on.
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut mi).as_bool() {
            warn_probe_throttled(format_args!(
                "pip: GetMonitorInfoW failed for cached hwnd {khwnd:#x} (display hot-plug in \
                 progress?) — cannot judge whether it outgrew its corner tile, so it stays \
                 topmost and tracked. Retrying every tick; throttled to one per 5s."
            ));
            continue;
        }
        let wa = mi.rcWork;

        if !should_release(
            zoomed,
            rect.right - rect.left,
            rect.bottom - rect.top,
            wa.right - wa.left,
            wa.bottom - wa.top,
        ) {
            continue;
        }

        // NATIVE_SAFETY row 1 — the shell is not an app. explorer.exe is one
        // process, so even a pid match can be a recycled handle onto shell
        // infrastructure (the desktop's Progman/WorkerW cover a whole monitor
        // and would trip the fullscreen test above). The ONLY explorer window
        // this app may act on is a real File Explorer window, CabinetWClass —
        // same rule and same check as smart_cascade's cached-HWND guard.
        let stem = crate::hook::exclusions::process_stem_for_pid(pid);
        if stem == "explorer" {
            let mut cls_buf = [0u16; 64];
            let n = GetClassNameW(hwnd, &mut cls_buf);
            let cls = String::from_utf16_lossy(&cls_buf[..n.max(0) as usize]);
            if cls != "CabinetWClass" {
                log::warn!(
                    "pip: cached hwnd {khwnd:#x} resolves to explorer.exe class '{cls}' — shell \
                     infrastructure. Dropping the PiP entry WITHOUT touching the window \
                     (NATIVE_SAFETY row 1)."
                );
                cache.lock().unwrap_or_else(|p| p.into_inner()).remove(&key);
                continue;
            }
        }

        // ── Which release is this? ───────────────────────────────────────
        //
        // FULL (the window is MAXIMISED): rcNormalPosition is not displaying
        // anything, so the true pre-PiP bounds can be written back into it
        // invisibly. Once Windows knows them again the cache entry is
        // redundant and is removed — the owner's Task 3 contract exactly.
        //
        // HALF (enlarged but NOT maximised, i.e. true fullscreen): the normal
        // rect IS the live geometry, so writing it would yank the window out
        // of fullscreen. Dropping topmost AND deleting the entry here would
        // leave the pre-PiP bounds nowhere at all — not in our cache, not in
        // Windows — which is the unrecoverable loss TASK 3b forbids, reached
        // by F11 instead of the maximise button. So: drop topmost (that IS
        // the reported bug), keep the entry as the last copy, and flag it.
        let Some(disposition) = release_disposition(zoomed, entry.state, entry.fullscreen_pip)
        else {
            // Already half-released and still not maximised — nothing new to
            // do. Silent by design: this arm is hit twice a second for as
            // long as the window stays fullscreen.
            continue;
        };

        // Claim under a short lock, MATCHING THE SNAPSHOT'S SERIAL. Between
        // the snapshot and here the engine can have restored this window (5th
        // tap, entry removed) and then entered PiP again (next tap, a brand-
        // new entry under the same key). A bare `remove` cannot tell that new
        // entry from the one we judged, and would release a PiP created
        // milliseconds ago — inside the very settle grace that exists to
        // protect it. Claiming BEFORE acting is still deliberate: a release
        // must never leave a window stuck topmost because a bookkeeping call
        // failed, so from here on every step is best-effort.
        let claimed = claim_for_release(
            &mut cache.lock().unwrap_or_else(|p| p.into_inner()),
            key,
            entry.serial,
            disposition,
        );
        let Some(entry) = claimed else {
            continue;
        };

        // Halt any in-flight animation with no parting corner snap — see
        // ANIM_CANCEL. (Reachable only if the user maximised mid-flight and
        // the grace period still elapsed; cheap insurance regardless.)
        ANIM_CANCEL.fetch_add(1, Ordering::SeqCst);

        let geometry = (
            rect.right - rect.left,
            rect.bottom - rect.top,
            wa.right - wa.left,
            wa.bottom - wa.top,
        );
        finish_release_off_thread(key, entry, disposition, zoomed, geometry, stem);
    }
}

/// What §7 should do with an entry it has decided is enlarged.
#[cfg(windows)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Disposition {
    /// Write the pre-PiP bounds back into `rcNormalPosition` and forget the
    /// window entirely. Only valid while the window is MAXIMISED.
    Full,
    /// Drop always-on-top but KEEP the cache entry: it is the last copy of
    /// the pre-PiP bounds and Windows cannot be handed them yet.
    HalfKeepEntry,
}

/// Pure (§7): take ownership of an entry the watcher has decided to release,
/// but ONLY if the entry still under `key` is the one that was judged.
///
/// The watcher decides on a snapshot and acts later. In between, the engine
/// can remove this entry (5th tap) and insert a brand-new one for the same
/// HWND (next tap). A bare `remove(&key)` returns that new entry and the
/// watcher goes on to release a PiP the user created milliseconds ago —
/// inside the settle grace, which was checked against the OLD entry and never
/// re-checked. Matching the serial is what makes the claim mean "the thing I
/// judged" instead of "whatever is here now".
#[cfg(windows)]
fn claim_for_release(
    map: &mut HashMap<PipKey, PipEntry>,
    key: PipKey,
    serial: u64,
    disposition: Disposition,
) -> Option<PipEntry> {
    match map.get_mut(&key) {
        Some(e) if e.serial == serial => match disposition {
            Disposition::Full => map.remove(&key),
            Disposition::HalfKeepEntry => {
                // Kept, not removed — it is the last copy of the pre-PiP
                // bounds. Marked Released so the next tick does not
                // re-process it AND so the engine stops treating it as a live
                // PiP: the next tap on the PiP key re-enters instead of
                // cycling a window that is no longer on top (§8).
                e.state = PipState::Released;
                Some(e.clone())
            }
        },
        _ => None,
    }
}

/// Pure: has a DIFFERENT PiP taken this window over since we claimed it?
/// A new entry under the same key means the engine re-entered PiP and has
/// already set the window HWND_TOPMOST — our HWND_NOTOPMOST would land last
/// and silently undo it.
#[cfg(windows)]
fn reentered_since_claim(map: &HashMap<PipKey, PipEntry>, key: PipKey, serial: u64) -> bool {
    map.get(&key).is_some_and(|e| e.serial != serial)
}

/// Pure (§7): given the window's zoomed state and the entry's own state, what
/// is left to do? `None` = nothing.
///
/// §9 ADDED THE THIRD ARGUMENT, and it is the watcher exemption trap 2 was
/// about. A `fullscreen_pip` entry's `original_*` is the MONITOR RECT — that
/// is what a fullscreen window measures as, by definition. `Disposition::Full`
/// means "hand those bounds to Windows as the window's `rcNormalPosition`",
/// and doing that would tell Windows the window's UN-MAXIMISED size is the
/// whole screen: a lie, permanently recorded, that the user would meet the
/// next time they un-maximised. So a fullscreen entry never gets a `Full`
/// release. It gets the half — topmost dropped, entry KEPT as the only copy
/// of the bounds — which is exactly what §7 already does for the case where
/// the bounds cannot honestly be handed back.
///
/// NOTE WHAT DOES *NOT* NEED AN EXEMPTION, because the fear was reasonable
/// and the measurement settled it. `should_release` looked like it would tear
/// this feature down on the first tick: it releases anything covering ≥75% of
/// the work area. But a fullscreen-PiP'd window's REAL bounds after the move
/// are the corner tile — measured at 1280x800 on a 2560x1600 work area, 25%,
/// with `IsZoomed` false — because "fullscreen" here is the app's own drawing
/// state, not a window rect. `should_release` has always measured actual
/// bounds rather than fullscreen state, so it keeps the PiP correctly and is
/// left exactly as it was.
#[cfg(windows)]
fn release_disposition(zoomed: bool, state: PipState, fullscreen_pip: bool) -> Option<Disposition> {
    if zoomed && !fullscreen_pip {
        // Maximised: the write-back is safe and finishes the job, whether or
        // not a half-release already happened for this window.
        Some(Disposition::Full)
    } else if state == PipState::Released {
        // Already half-released and still not maximised. Returning anything
        // here would re-drop topmost and re-toast on EVERY 500 ms tick for as
        // long as the window stays fullscreen.
        None
    } else {
        Some(Disposition::HalfKeepEntry)
    }
}

/// The blocking half of a release, on a disposable thread.
///
/// `SetWindowPlacement` and a z-order `SetWindowPos` are SendMessage-class
/// against a foreign window: they do not return until the target's WndProc
/// has run them, with no timeout. The only caller is the fullscreen watcher,
/// which is the SOLE writer of `hook::FULLSCREEN_ACTIVE` — freeze it and
/// either every shortcut in the app dies or they all fire inside a game
/// (PROBLEM 88). §6 moved the entry path's blocking call off the engine
/// thread for the same reason; this is the release side of that.
///
/// The entry has already been claimed, so nothing here can race the cache.
#[cfg(windows)]
fn finish_release_off_thread(
    key: PipKey,
    entry: PipEntry,
    disposition: Disposition,
    zoomed: bool,
    geometry: (i32, i32, i32, i32),
    stem: String,
) {
    // Cloned for the thread so the originals survive for the fallback below —
    // `Builder::spawn` consumes its closure and drops it when the spawn fails.
    let (t_entry, t_stem) = (entry.clone(), stem.clone());
    match std::thread::Builder::new()
        .name("st-pip-release".into())
        .spawn(move || run_release(key, t_entry, disposition, zoomed, geometry, t_stem))
    {
        Ok(_) => {}
        Err(e) => {
            // PROBLEM 124 — a spawn failure is plausible on someone else's
            // machine and must degrade, not disappear. Degraded means "the
            // watcher takes the blocking exposure for this one release",
            // which is strictly better than a window left stuck on top.
            log::warn!(
                "pip: could not spawn the release thread ({e}) — completing the release of \
                 hwnd {:#x} inline on the watcher thread instead",
                key.hwnd
            );
            run_release(key, entry, disposition, zoomed, geometry, stem);
        }
    }
}

/// The body of a release. Runs on `st-pip-release`, or inline on the watcher
/// thread if that thread could not be spawned.
#[cfg(windows)]
fn run_release(
    key: PipKey,
    entry: PipEntry,
    disposition: Disposition,
    zoomed: bool,
    geometry: (i32, i32, i32, i32),
    stem: String,
) {
    let hwnd = windows::Win32::Foundation::HWND(key.hwnd as *mut _);
    let khwnd = key.hwnd;
    let (win_w, win_h, work_w, work_h) = geometry;

    // §7/TASK 3b — the write-back. PiP placed the window at the corner while
    // it was in its NORMAL state, so Windows recorded the corner tile as its
    // rcNormalPosition and FORGOT the true pre-PiP bounds; this entry is the
    // last copy. Writing rcNormalPosition while the window is maximised is
    // invisible — the field is not displaying anything — and takes effect
    // exactly when the user un-maximises.
    // §10 — a release means the window has been enlarged out from under PiP
    // and is allowed to be big. Disarm before anything else, or the guard would
    // keep hauling it back to a corner it is no longer supposed to occupy.
    if entry.fullscreen_pip {
        disarm_click_guard("the PiP was released after the window was enlarged");
    }

    let wrote_back = match disposition {
        Disposition::Full => unsafe { write_back_normal_position(hwnd, &entry) },
        Disposition::HalfKeepEntry => false,
    };

    // The engine may have entered PiP again for this same window while we were
    // in there — the cache has been ours to lose since the claim. A different
    // serial under our key means a NEW PiP owns the window and has just set it
    // HWND_TOPMOST; dropping topmost now would land last and leave a window
    // that is tracked as PiP, animating to a corner and captioned
    // "PiP: Top-Left", but not actually on top of anything.
    let superseded = reentered_since_claim(
        &global_cache().lock().unwrap_or_else(|p| p.into_inner()),
        key,
        entry.serial,
    );

    if superseded {
        log::info!(
            "pip: hwnd {khwnd:#x} was re-entered into PiP while its release was in flight — \
             leaving the new entry's HWND_TOPMOST alone (rcNormalPosition write-back: {})",
            if wrote_back { "written" } else { "not written" }
        );
        return;
    }

    // Drop always-on-top and leave the window exactly where the user put it —
    // no move, no size, no activation. This runs even when the write-back
    // failed: a window stuck above everything is the bug this exists to fix,
    // and bookkeeping must never preserve it.
    //
    // SWP_ASYNCWINDOWPOS: post the z-order change to the target's own thread
    // instead of waiting on it. We are on a disposable thread, so blocking
    // here would cost nothing but a leaked wait — but this is the ONE call
    // that must not be skipped, and making it unblockable keeps that true on
    // the inline fallback path too.
    unsafe {
        use windows::Win32::UI::WindowsAndMessaging::{
            SetWindowPos, HWND_NOTOPMOST, SWP_ASYNCWINDOWPOS, SWP_NOACTIVATE, SWP_NOMOVE,
            SWP_NOSIZE,
        };
        let _ = SetWindowPos(
            hwnd,
            HWND_NOTOPMOST,
            0,
            0,
            0,
            0,
            SWP_ASYNCWINDOWPOS | SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }

    match disposition {
        Disposition::Full => log::info!(
            "pip: released hwnd {khwnd:#x} ({stem}) — grew beyond its corner tile \
             (zoomed={zoomed}, {win_w}x{win_h} on a {work_w}x{work_h} work area). \
             Topmost dropped, entry removed, rcNormalPosition write-back: {}",
            if wrote_back { "written" } else { "skipped/failed (see above)" }
        ),
        Disposition::HalfKeepEntry => log::info!(
            "pip: half-released hwnd {khwnd:#x} ({stem}) — it covers {win_w}x{win_h} of a \
             {work_w}x{work_h} work area but is NOT maximised (true fullscreen), so \
             rcNormalPosition is live geometry and rewriting it would move the window. \
             Topmost dropped; the cache entry is KEPT because it is the only surviving \
             copy of the pre-PiP bounds (TASK 3b). The 5th tap and restore-on-exit can \
             still recover it, and maximising this window later completes the release."
        ),
    }

    // Toast wording follows what actually happened — the user's window is
    // large right now and they need to know whether their original size
    // survived. Single line, leading glyph becomes the icon, same length class
    // as the existing pip toasts.
    let msg = match (disposition, wrote_back) {
        (Disposition::Full, true) => "📺 PiP released — un-maximize for original size",
        (Disposition::Full, false) => "📺 PiP released",
        (Disposition::HalfKeepEntry, _) => "📺 PiP: always-on-top released",
    };
    // Same dispatch the watcher thread used to do inline: AppHandle::emit is
    // fire-and-forget onto the main event loop, not a blocking getter, so it
    // is safe from any thread.
    if let Some(handle) = crate::guide_hud::app_handle() {
        crate::show_toast(&handle, msg);
    }
}

/// TASK 3b — write the cached pre-PiP bounds back into the window's own
/// `rcNormalPosition`, so Windows itself once again knows where the window
/// belongs and a later un-maximise lands on the true original frame.
///
/// Returns true only when the write went through. Failure is survivable by
/// design — the caller drops topmost and forgets the entry regardless — but
/// it is logged, because a failed write here means the next un-maximise will
/// land on the corner tile.
#[cfg(windows)]
unsafe fn write_back_normal_position(
    hwnd: windows::Win32::Foundation::HWND,
    entry: &PipEntry,
) -> bool {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::Graphics::Gdi::{MonitorFromWindow, MONITOR_DEFAULTTONEAREST};
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowPlacement, IsZoomed, SetWindowPlacement, WINDOWPLACEMENT, WINDOWPLACEMENT_FLAGS,
    };

    // The write is invisible ONLY while the window is maximised. On a
    // normal-state window `SetWindowPlacement` applies rcNormalPosition as
    // the window's live position, i.e. it MOVES it — that is the documented
    // behaviour of the call, not an observation from this machine. So this
    // re-checks IsZoomed at the last instant; a showCmd read from an earlier
    // GetWindowPlacement would not survive the user un-maximising in the gap,
    // and that gap is now a thread hop wide.
    //
    // Reaching here not-zoomed is a SKIP, not a release: the caller has
    // classified this as `Disposition::HalfKeepEntry` and is keeping the
    // cache entry precisely because the bounds could not be handed back.
    // The one case that still lands here as a `Full` release is the user
    // un-maximising between the watcher's decision and this call — the window
    // is back at its corner tile, PiP-shaped, and skipping is the least-wrong
    // answer for a race that narrow.
    if !IsZoomed(hwnd).as_bool() {
        log::info!(
            "pip: hwnd {:#x} is not maximised at write-back time (true fullscreen, or it was \
             un-maximised in the race window) — skipping the rcNormalPosition write; writing \
             it in the normal state would visibly move the window",
            hwnd.0 as isize
        );
        return false;
    }

    let mut wp = WINDOWPLACEMENT {
        length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
        ..Default::default()
    };
    if GetWindowPlacement(hwnd, &mut wp).is_err() {
        log::warn!(
            "pip: GetWindowPlacement failed for hwnd {:#x} during release (elevated target?) — \
             cannot write the original bounds back; un-maximise will land on the corner tile",
            hwnd.0 as isize
        );
        return false;
    }

    // rcNormalPosition is in WORKSPACE coordinates — screen coordinates minus
    // the work area's inset within its own monitor. That inset is 48 px
    // vertically on this machine (probed 2026-08-26: primary rcMonitor
    // (0,0)-(2560,1600) vs rcWork (0,48)-(2560,1600)), so the conversion is
    // load-bearing here, not theoretical.
    //
    // THE INSET COMES FROM `MonitorFromWindow(hwnd)`, i.e. the monitor the
    // window is on RIGHT NOW. It used to be resolved from the ORIGINAL bounds
    // instead, justified as "PiP moves windows to the CURSOR's monitor (§4),
    // so the original may belong to monitor A while the window sits on B —
    // using B's inset would bake (insetB − insetA) into the restore". That
    // reasoning is exactly inverted. We are ENCODING a value that Windows
    // will DECODE later, and Windows decodes it against the window's own
    // monitor — the same basis `normal_position_to_screen` reads it with, and
    // the same one Chromium's HWNDMessageHandler::GetWindowPlacement uses,
    // which is the only evidence either direction has. Encoding with a
    // monitor the OS will not use is what bakes in (insetB − insetA): the
    // window comes back 48 px off, and each PiP → maximise → release cycle
    // re-captures the shifted rect and creeps another 48 px, the exact creep
    // the WINDOWPLACEMENT docs warn about.
    //
    // On one display, or whenever the original and the window are on the same
    // monitor, both rules resolve to the same HMONITOR and this change is a
    // no-op — which is also why no single-display hand test can tell them
    // apart, and why the round-trip check below cannot either.
    let (inset_x, inset_y) =
        monitor_workspace_inset(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST)).unwrap_or_else(
            || {
                log::warn!(
                    "pip: GetMonitorInfoW failed resolving the workspace inset for hwnd {:#x} — \
                     assuming (0,0), which is right for a bottom-only taskbar and 48px wrong here",
                    hwnd.0 as isize
                );
                (0, 0)
            },
        );
    let (l, t, r, b) = rc_normal_for_release(
        entry.original_x,
        entry.original_y,
        entry.original_w,
        entry.original_h,
        inset_x,
        inset_y,
    );

    wp.length = std::mem::size_of::<WINDOWPLACEMENT>() as u32;
    // Explicit zero rather than echoing what was read. WPF_RESTORETOMAXIMIZED
    // and WPF_SETMINPOSITION both change what SetWindowPlacement DOES, and
    // neither is anything we mean to ask for — we are editing one rect, not
    // commanding a restore policy. UNVERIFIED ON THIS MACHINE: nobody has
    // watched a real window through this call, so if a maximised window ever
    // comes back un-maximised after a release, this line is the first suspect.
    wp.flags = WINDOWPLACEMENT_FLAGS(0);
    // showCmd is left EXACTLY as read — commanding SW_SHOWMAXIMIZED here
    // would be a state change with ShowWindow semantics. Echoing the current
    // state is the closest thing to a no-op the API offers; it is NOT
    // guaranteed to be message-free, which is one reason this whole function
    // runs off the watcher thread.
    wp.rcNormalPosition = RECT { left: l, top: t, right: r, bottom: b };

    if SetWindowPlacement(hwnd, &wp).is_err() {
        log::warn!(
            "pip: SetWindowPlacement failed for hwnd {:#x} (elevated target?) — original \
             bounds NOT written back; un-maximise will land on the corner tile",
            hwnd.0 as isize
        );
        return false;
    }

    // WHAT THIS CHECK CAN AND CANNOT SEE. It compares the bytes we wrote with
    // the bytes that come back, and Set/Get encode and decode against the SAME
    // monitor — so the round trip is the identity NO MATTER WHICH INSET WE
    // USED. It therefore cannot validate the coordinate space; a wrong-monitor
    // write would sail through it and the log line would call it fine. An
    // earlier version of this comment claimed a mismatch "means a monitor
    // disagreement", which is precisely the one thing it does not mean.
    //
    // What it CAN catch is the OS refusing or altering the rect — chiefly
    // SetWindowPlacement's silent visibility clamp when the original bounds
    // belong to a display that has since been unplugged, which is routine on
    // this machine. Log it, never loop on it: the clamp IS the desired
    // behaviour for a vanished display.
    //
    // Catching a coordinate-space error needs a rect read back AFTER a real
    // un-maximise, which only a hand test on a two-display setup can do.
    let mut back = WINDOWPLACEMENT {
        length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
        ..Default::default()
    };
    if GetWindowPlacement(hwnd, &mut back).is_ok() {
        let got = back.rcNormalPosition;
        if got.left != l || got.top != t || got.right != r || got.bottom != b {
            log::warn!(
                "pip: rcNormalPosition write-back did not round-trip — wrote ({l},{t})-({r},{b}), \
                 read back ({},{})-({},{}). SetWindowPlacement altered the rect; the usual cause \
                 is its clamp to keep the window visible (a display may have been unplugged). \
                 Not retrying. NOTE this check cannot detect a wrong-inset write — see the \
                 comment above it.",
                got.left, got.top, got.right, got.bottom
            );
        }
    }

    true
}

/// Pure decision (§7): has a PiP'd window grown beyond its corner tile?
///
/// `zoomed` — `IsZoomed`, catches maximise (and fullscreen-by-maximise).
/// The area test catches TRUE fullscreen, which is not "zoomed": a borderless
/// fullscreen window covers rcMonitor, a superset of the work area, so its
/// area is ≥ 100% of the work area's.
///
/// THE THRESHOLD: release when the window's area reaches 75% of the work
/// area. The corner tile is 50%×50% = 25%, so a user would have to TRIPLE the
/// tile's area before crossing it — no accidental nudge or modest manual
/// resize gets near that, which is the conservative direction the owner asked
/// for ("a user nudging the window slightly larger must NOT lose PiP").
/// Meanwhile every enlargement that matters clears it with margin: maximise
/// is caught by `zoomed` before area is even consulted, and fullscreen sits
/// at ≥100%. A wrong "keep" merely leaves today's behaviour (still topmost —
/// annoying, recoverable by cycling out); the gap between 25% and 75% is what
/// keeps the wrong "release" unreachable.
///
/// Degenerate inputs (a failed monitor read, an empty rect) fail toward NOT
/// releasing — the status quo — rather than toward acting on bad data.
#[cfg(windows)]
fn should_release(zoomed: bool, win_w: i32, win_h: i32, work_w: i32, work_h: i32) -> bool {
    if zoomed {
        return true;
    }
    if win_w <= 0 || win_h <= 0 || work_w <= 0 || work_h <= 0 {
        return false;
    }
    // i64: 4K monitors already put i32 pixel-area products near 8.3M×… well
    // within range, but width×height×4 on a hypothetical giant virtual screen
    // is not worth an overflow footnote.
    let win_area = win_w as i64 * win_h as i64;
    let work_area = work_w as i64 * work_h as i64;
    win_area * 4 >= work_area * 3
}

/// Pure (§7/TASK 3b): the rect to store in `rcNormalPosition` for a window
/// whose pre-PiP SCREEN bounds were (x, y, w, h), on a monitor whose work
/// area is inset by (inset_x, inset_y) from the monitor's own origin.
/// Workspace = screen − inset, applied to both corners; width and height are
/// untouched by construction.
#[cfg(windows)]
fn rc_normal_for_release(
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    inset_x: i32,
    inset_y: i32,
) -> (i32, i32, i32, i32) {
    (x - inset_x, y - inset_y, x + w - inset_x, y + h - inset_y)
}

/// The workspace↔screen offset of monitor `hmon`: the work area's inset
/// within its own monitor, `(rcWork.left − rcMonitor.left, rcWork.top −
/// rcMonitor.top)`. NOT the absolute rcWork origin — a display parked at
/// (−1920, 0) contributes nothing here; only a taskbar or appbar docked to
/// that monitor's TOP or LEFT edge does.
///
/// Probed on this machine 2026-08-26 (EnumDisplayMonitors + GetMonitorInfoW):
/// the primary reports rcMonitor (0,0)-(2560,1600) and rcWork
/// (0,48)-(2560,1600) — a bar docked across the TOP, inset (0, 48) in
/// physical pixels. So do not delete this conversion because a test run on a
/// stock bottom-taskbar layout reports (0, 0); that is the one configuration
/// where its absence is invisible.
#[cfg(windows)]
unsafe fn monitor_workspace_inset(
    hmon: windows::Win32::Graphics::Gdi::HMONITOR,
) -> Option<(i32, i32)> {
    use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITORINFO};
    let mut mi = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if !GetMonitorInfoW(hmon, &mut mi).as_bool() {
        return None;
    }
    // physical pixels
    Some((
        mi.rcWork.left - mi.rcMonitor.left,
        mi.rcWork.top - mi.rcMonitor.top,
    ))
}

/// READ direction (§6): `rcNormalPosition` (workspace coords) → screen
/// coords, safe to hand to `SetWindowPos` later. Pure translation — width and
/// height are never altered.
///
/// The inset comes from the WINDOW's monitor and STOPS THERE, which is what
/// Chromium's `HWNDMessageHandler::GetWindowPlacement` does and the only
/// model with any evidence behind it: Windows stored this rect against the
/// window's own monitor, so that is the monitor that decodes it.
///
/// This used to convert once, then re-resolve the monitor from the CONVERTED
/// rect and redo the shift with that monitor's inset if it differed. It was
/// labelled a safety net and was the opposite. The only way to reach the
/// re-resolve is a rect that lands on a different display than the window —
/// e.g. a maximised window on the primary whose remembered normal bounds are
/// on a second monitor, which session restore and Win+Shift+Arrow both
/// produce. In exactly that case Windows encoded against the WINDOW's
/// monitor, so redoing the shift with the other monitor's inset manufactures
/// the coordinate error the net was supposed to catch: `PipEntry.original_y`
/// ends up 48 px out, the 5th tap restores the window 48 px too high, and the
/// release write-back then persists the wrong rect. There is no hypothesis
/// under which the second answer is the OS's encoding.
#[cfg(windows)]
unsafe fn normal_position_to_screen(
    r: windows::Win32::Foundation::RECT,
    hwnd: windows::Win32::Foundation::HWND,
) -> windows::Win32::Foundation::RECT {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::Graphics::Gdi::{MonitorFromWindow, MONITOR_DEFAULTTONEAREST};

    let Some((dx, dy)) =
        monitor_workspace_inset(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST))
    else {
        log::warn!("pip: GetMonitorInfoW failed resolving the workspace inset — assuming (0,0)");
        return r;
    };
    RECT {
        left: r.left + dx,
        top: r.top + dy,
        right: r.right + dx,
        bottom: r.bottom + dy,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corners_are_the_four_corners_of_the_work_area() {
        // A 1920x1040 work area at origin (0,0): tiles are 960x520.
        let (l, t, w, h) = (0, 0, 1920, 1040);
        let (pw, ph) = (w / 2, h / 2);
        assert_eq!(corner_position(0, l, t, pw, ph, w, h), (0, 0));
        assert_eq!(corner_position(1, l, t, pw, ph, w, h), (960, 0));
        assert_eq!(corner_position(2, l, t, pw, ph, w, h), (960, 520));
        assert_eq!(corner_position(3, l, t, pw, ph, w, h), (0, 520));
    }

    #[test]
    fn corners_respect_a_non_zero_monitor_origin() {
        // A second display to the LEFT of the primary has negative coordinates,
        // and a taskbar makes the work area's top non-zero. Both were correct
        // before; this pins them so a future "simplification" cannot regress
        // the two-monitor case the owner actually runs.
        let (l, t, w, h) = (-1920, 48, 1920, 992);
        let (pw, ph) = (w / 2, h / 2);
        assert_eq!(corner_position(0, l, t, pw, ph, w, h), (-1920, 48));
        assert_eq!(corner_position(1, l, t, pw, ph, w, h), (-960, 48));
        assert_eq!(corner_position(2, l, t, pw, ph, w, h), (-960, 544));
        assert_eq!(corner_position(3, l, t, pw, ph, w, h), (-1920, 544));
    }

    /// The vertical hop is the one the old exit test broke on. This asserts
    /// the SHAPE of the bug rather than the animation: corner 1 → corner 2
    /// changes only Y, so any convergence test that ignores Y is satisfied
    /// before the window has moved.
    #[test]
    fn top_right_to_bottom_right_is_a_purely_vertical_move() {
        let (l, t, w, h) = (0, 0, 1920, 1040);
        let (pw, ph) = (w / 2, h / 2);
        let a = corner_position(1, l, t, pw, ph, w, h);
        let b = corner_position(2, l, t, pw, ph, w, h);
        assert_eq!(a.0, b.0, "x must not change on this hop");
        assert_ne!(a.1, b.1, "y must change on this hop");
    }

    // ─────────────────────────────────────────────────────────────────────
    // §10 / PROBLEM 220 — the two PiP keys have INDEPENDENT state.
    //
    // The owner tested 1.0.93 after being warned that Space+` and Space+Tab
    // shared one `PipEntry` map, and reported: *"it did behave oddly."* These
    // pin the separation and the takeover rule that keeps it coherent.
    // ─────────────────────────────────────────────────────────────────────

    #[test]
    fn one_window_can_hold_a_separate_entry_under_each_key() {
        // The core of the separation: same HWND, two namespaces, and NEITHER
        // may see the other's bounds, corner index or fullscreen capture.
        let mut map: HashMap<PipKey, PipEntry> = HashMap::new();
        map.insert(ck(0xA), entry(10, 20, 300, 400, 1));
        map.insert(fk(0xA), fs_entry(700, 800, 900, 1000, 2));

        assert_eq!(map.len(), 2, "one HWND, two keys, two entries");
        let corner = map.get(&ck(0xA)).expect("Space+` entry");
        let fs = map.get(&fk(0xA)).expect("Space+Tab entry");
        assert_eq!((corner.original_x, corner.original_w), (10, 300));
        assert_eq!((fs.original_x, fs.original_w), (700, 900));
        assert!(!corner.fullscreen_pip, "Space+` never acquires the flag");
        assert!(fs.fullscreen_pip);
        assert!(
            corner.fullscreen_state.is_none() && fs.fullscreen_state.is_some(),
            "the fullscreen CAPTURE must not leak across the two keys either — it is what \
             the 5th tap replays"
        );
    }

    #[test]
    fn cycling_one_key_does_not_advance_the_other_keys_corner() {
        // The position index was shared in 1.0.93, so tapping either key moved
        // whichever entry happened to be under the bare HWND. That is a large
        // part of "it did behave oddly".
        let mut map: HashMap<PipKey, PipEntry> = HashMap::new();
        map.insert(ck(0xA), entry(10, 20, 300, 400, 1));
        map.insert(fk(0xA), fs_entry(10, 20, 300, 400, 2));

        assert_eq!(tap_for(&mut map, ck(0xA), 4242), Tap::Cycle(1));
        assert_eq!(tap_for(&mut map, ck(0xA), 4242), Tap::Cycle(2));
        assert_eq!(
            map.get(&fk(0xA)).map(|e| e.position_index),
            Some(0),
            "two taps of Space+` must leave Space+Tab's corner index untouched"
        );
        // And the reverse.
        assert_eq!(tap_for(&mut map, fk(0xA), 4242), Tap::Cycle(1));
        assert_eq!(
            map.get(&ck(0xA)).map(|e| e.position_index),
            Some(2),
            "and a Space+Tab tap must not disturb Space+`'s"
        );
    }

    #[test]
    fn the_fifth_tap_of_one_key_leaves_the_other_keys_entry_alone() {
        // The 5th tap REMOVES an entry. Under one shared map it removed the
        // only entry there was, so the other feature silently lost its window.
        let mut map: HashMap<PipKey, PipEntry> = HashMap::new();
        map.insert(ck(0xA), entry(10, 20, 300, 400, 1));
        map.insert(fk(0xA), fs_entry(700, 800, 900, 1000, 2));
        for _ in 0..3 {
            tap_for(&mut map, ck(0xA), 4242);
        }
        assert!(matches!(tap_for(&mut map, ck(0xA), 4242), Tap::Restore(_)));
        assert!(!map.contains_key(&ck(0xA)), "Space+` let go of the window");
        assert_eq!(
            map.get(&fk(0xA)).map(|e| e.original_x),
            Some(700),
            "and Space+Tab's entry — with its own bounds — survived intact"
        );
    }

    #[test]
    fn the_second_key_takes_the_window_over_and_hands_back_what_to_restore() {
        // THE TAKEOVER RULE. Never two live entries for one HWND: the second
        // key claims the first key's entry so the caller can put the window
        // back the way that feature found it BEFORE measuring it afresh.
        let mut map: HashMap<PipKey, PipEntry> = HashMap::new();
        map.insert(ck(0xA), entry(10, 20, 300, 400, 1));

        let victim = takeover_victim(&mut map, 0xA, PipMode::FullscreenPreserving)
            .expect("Space+Tab must claim the window Space+` was holding");
        assert_eq!(
            (victim.original_x, victim.original_y, victim.original_w, victim.original_h),
            (10, 20, 300, 400),
            "and hand back the ORIGINAL frame, which is what gets restored"
        );
        assert!(
            map.is_empty(),
            "the other key's entry is REMOVED by the claim — a window is held by one key at \
             a time, and a surviving second entry is exactly what produced \"it did behave \
             oddly\""
        );
    }

    #[test]
    fn a_takeover_claims_only_the_other_key_and_only_for_that_window() {
        let mut map: HashMap<PipKey, PipEntry> = HashMap::new();
        map.insert(ck(0xA), entry(10, 20, 300, 400, 1));
        map.insert(ck(0xB), entry(50, 60, 70, 80, 2));

        // Same key as the entry: nothing to take over — that is a plain cycle.
        assert!(
            takeover_victim(&mut map, 0xA, PipMode::Corner).is_none(),
            "Space+` tapping a window Space+` already holds is a CYCLE, not a takeover"
        );
        assert!(map.contains_key(&ck(0xA)), "and it must not be removed");

        // A different window's entry is none of this tap's business.
        assert!(takeover_victim(&mut map, 0xC, PipMode::FullscreenPreserving).is_none());
        assert_eq!(map.len(), 2, "no other window's entry may be disturbed");
    }

    #[test]
    fn a_takeover_carries_no_state_from_the_key_it_replaced() {
        // After the takeover the new key measures the (restored) window and
        // starts at corner 0 with its own serial. What must NOT happen is the
        // new entry inheriting the old one's corner index or fullscreen
        // capture — the two features' state is independent by construction.
        let mut map: HashMap<PipKey, PipEntry> = HashMap::new();
        let mut held = fs_entry(700, 800, 900, 1000, 5);
        held.position_index = 3;
        map.insert(fk(0xA), held);

        let victim = takeover_victim(&mut map, 0xA, PipMode::Corner).expect("claimed");
        assert_eq!(victim.position_index, 3, "the claim hands back what it found");
        // The Corner namespace is untouched by the claim, so the tap that
        // follows it is a genuine first entry — measured, corner 0, no capture.
        assert_eq!(tap_for(&mut map, ck(0xA), 4242), Tap::Enter(None));
    }

    #[test]
    fn prune_dead_prunes_both_namespaces() {
        // One `retain` over the one map — the argument for putting the mode in
        // the key. A second container would need a second prune, and a dead
        // HWND left in the survivor is one a recycled handle inherits
        // (NATIVE_SAFETY rule 3).
        use windows::Win32::UI::WindowsAndMessaging::GetDesktopWindow;
        // A REAL live window as the control, so this test can produce a
        // negative: without it, "everything was dropped" would pass even if
        // prune_dead dropped the whole map unconditionally. IsWindow is a
        // read of the window manager's own table — nothing is acted on here.
        let alive = unsafe { GetDesktopWindow() }.0 as isize;
        let dead = 0x7FFF_FFF0isize;

        let mut map: HashMap<PipKey, PipEntry> = HashMap::new();
        map.insert(ck(alive), entry(1, 2, 3, 4, 1));
        map.insert(fk(alive), entry(1, 2, 3, 4, 2));
        map.insert(ck(dead), entry(1, 2, 3, 4, 3));
        map.insert(fk(dead), entry(1, 2, 3, 4, 4));

        prune_dead(&mut map);

        assert!(
            map.contains_key(&ck(alive)) && map.contains_key(&fk(alive)),
            "a live window must survive the prune in BOTH namespaces"
        );
        assert!(
            !map.contains_key(&ck(dead)) && !map.contains_key(&fk(dead)),
            "and a dead one must be dropped from BOTH — pruning only one namespace leaves a \
             stale HWND for a recycled handle to inherit"
        );
    }

    #[test]
    fn restore_all_drains_both_namespaces() {
        // The failure this forbids is PROBLEM 167's orphan: quit Spaceadom and
        // a window it never let go of stays pinned topmost at quarter size with
        // nothing left that can release it. A drain that saw only one of two
        // maps would do exactly that to whichever feature it missed.
        //
        // The HWNDs are deliberately dead, so `restore_all` finds `IsWindow`
        // false and touches no window at all — this exercises the DRAIN, which
        // is the part that can be written wrong.
        let _serialise = GLOBAL_CACHE_TESTS
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let cache = new_cache();
        {
            let mut map = cache.lock().unwrap_or_else(|p| p.into_inner());
            map.clear();
            map.insert(ck(0x7FFF_FF01), entry(1, 2, 3, 4, 101));
            map.insert(fk(0x7FFF_FF01), fs_entry(5, 6, 7, 8, 102));
            map.insert(fk(0x7FFF_FF02), fs_entry(9, 10, 11, 12, 103));
            assert_eq!(map.len(), 3);
        }

        restore_all();

        assert!(
            cache
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .is_empty(),
            "restore_all must drain EVERY entry of BOTH keys — anything it leaves behind is a \
             window nothing on the machine can un-PiP any more"
        );
    }

    /// §10 — the click guard's re-assert must carry `SWP_NOSENDCHANGING`.
    ///
    /// Structural, and it has to be: `click_guard_proc` is a Win32 WinEvent
    /// callback that needs a real fullscreen window and a real hook to run at
    /// all. What can still be asserted from here is the one thing that would
    /// silently break it — re-asserting with ORDINARY flags, which §9 measured
    /// being vetoed and reverted within 40 ms. The guard would then fire
    /// forever, achieve nothing, and trip its own runaway detector.
    #[test]
    fn the_click_guard_reasserts_with_the_veto_suppressed() {
        let src = include_str!("pip.rs").replace("\r\n", "\n");
        let start = src
            .find("unsafe extern \"system\" fn click_guard_proc(")
            .expect("the click guard callback must still exist");
        let end = start
            + src[start..]
                .find("\n/// The guard thread")
                .expect("the callback must still end before the guard thread");
        let body = &src[start..end];
        assert!(
            body.contains("move_flags(true)"),
            "the guard must re-assert with move_flags(true) — SWP_NOSENDCHANGING is the only \
             reason a fullscreen Chromium window accepts the tile at all (§9's measurement)"
        );
        assert!(
            body.contains("placement_held("),
            "and it must compare against the target with the same slack the entry \
             verification uses, or DPI rounding alone would make it correct forever"
        );
    }

    /// §10 — the 5th tap MUST disarm the guard before it places anything.
    ///
    /// This is the sharpest edge in the whole amendment: `restore_window` puts a
    /// fullscreen entry back to the WHOLE MONITOR, and a guard still armed reads
    /// that as drift and hauls the window straight back into the corner. The
    /// user would press the 5th tap and see nothing happen.
    #[test]
    fn the_restore_disarms_the_click_guard_before_it_moves_anything() {
        let src = include_str!("pip.rs").replace("\r\n", "\n");
        let start = src
            .find("unsafe fn restore_window(")
            .expect("restore_window must still exist");
        let body = &src[start..];
        let disarm = body
            .find("disarm_click_guard(")
            .expect("restore_window must disarm the click guard");
        let first_place = body
            .find("SetWindowPos(")
            .expect("restore_window must still place the window");
        assert!(
            disarm < first_place,
            "the disarm must come BEFORE any placement — otherwise the guard sees the restore \
             as drift and drags the window back into its corner tile, and the 5th tap looks \
             like it does nothing"
        );
    }

    #[test]
    fn the_cache_is_shared_not_per_caller() {
        let _serialise = GLOBAL_CACHE_TESTS
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        // restore_all() reads the global map from the exit handler, which has
        // no engine handle. If new_cache() ever goes back to handing out fresh
        // maps, shutdown silently restores nothing.
        let a = new_cache();
        let b = new_cache();
        a.lock().unwrap().insert(ck(0x1234), entry(1, 2, 3, 4, 7));
        assert!(
            b.lock().unwrap().contains_key(&ck(0x1234)),
            "new_cache() must hand out the SAME map, or restore_all() sees nothing"
        );
        a.lock().unwrap().clear();
    }

    /// The Space+` namespace key for a window (§10).
    fn ck(hwnd: isize) -> PipKey {
        PipKey {
            hwnd,
            mode: PipMode::Corner,
        }
    }

    /// The Space+Tab namespace key for the SAME window (§10). The whole point
    /// of PROBLEM 220 is that `ck(h)` and `fk(h)` are different entries.
    fn fk(hwnd: isize) -> PipKey {
        PipKey {
            hwnd,
            mode: PipMode::FullscreenPreserving,
        }
    }

    /// Serialises the two tests that use the PROCESS-WIDE cache. `restore_all`
    /// drains it, so without this the drain test and the shared-cache test race
    /// each other into intermittent failures — and a flaky test is worse than
    /// no test, because the next person learns to re-run it instead of reading
    /// it (CLAUDE.md: a check that cannot produce a trustworthy negative is not
    /// a check).
    static GLOBAL_CACHE_TESTS: Mutex<()> = Mutex::new(());

    /// Test-only constructor so the field list lives in one place.
    fn entry(x: i32, y: i32, w: i32, h: i32, serial: u64) -> PipEntry {
        PipEntry {
            original_x: x,
            original_y: y,
            original_w: w,
            original_h: h,
            was_maximized: false,
            position_index: 0,
            pid: 0,
            entered_at: 0,
            serial,
            state: PipState::Active,
            fullscreen_pip: false,
            fullscreen_state: None,
        }
    }

    /// A Space+Tab entry, as the probe would have built it: the flag AND the
    /// captured fullscreen state, because the restore leg replays the capture
    /// rather than re-deriving fullscreen from `original_*`.
    ///
    /// `original_*` here is deliberately the caller's, NOT the monitor rect.
    /// The first cut of §9 assumed those were the same thing; the owner's log
    /// showed they are not, and every test below that matters exercises the
    /// case where they differ.
    fn fs_entry(x: i32, y: i32, w: i32, h: i32, serial: u64) -> PipEntry {
        PipEntry {
            fullscreen_pip: true,
            fullscreen_state: Some(FullscreenState {
                style: STYLE_FULLSCREEN,
                ex_style: EXSTYLE_FULLSCREEN,
                x: MON.0,
                y: MON.1,
                w: MON.2 - MON.0,
                h: MON.3 - MON.1,
            }),
            ..entry(x, y, w, h, serial)
        }
    }

    // ── §7 release decision — the branch a user only reaches after their
    // window has already been enlarged out from under PiP (house rule: test
    // the pure logic behind exactly that kind of branch). Work area below is
    // the classic 1920x1040 (1080p minus a 40px taskbar); the corner tile is
    // therefore 960x520.

    #[test]
    fn a_corner_tile_is_not_released() {
        assert!(!should_release(false, 960, 520, 1920, 1040));
    }

    #[test]
    fn a_maximized_window_is_released() {
        // `zoomed` short-circuits — pass tile geometry to prove the flag alone
        // decides (a maximised window's own rect is irrelevant to the verdict).
        assert!(should_release(true, 960, 520, 1920, 1040));
    }

    #[test]
    fn a_fullscreen_sized_window_is_released() {
        // True fullscreen covers rcMonitor (1920x1080 here), a SUPERSET of the
        // work area — its area lands at ≥100% of the work area's, over the 75%
        // line with margin, and it is NOT zoomed, which is why the area test
        // exists at all.
        assert!(should_release(false, 1920, 1080, 1920, 1040));
    }

    #[test]
    fn a_slightly_resized_tile_keeps_pip() {
        // The owner's constraint: nudging the tile bigger must NOT lose PiP.
        // +120px on each axis — an aggressive manual resize — is still ~35%
        // of the work area, nowhere near the 75% line.
        assert!(!should_release(false, 1080, 640, 1920, 1040));
        // Even DOUBLING the tile's area (~50% of the work area) keeps PiP.
        assert!(!should_release(false, 1358, 736, 1920, 1040));
        // And the boundary itself: 75% exactly releases, a hair under keeps.
        assert!(should_release(false, 1000, 750, 1000, 1000));
        assert!(!should_release(false, 1000, 749, 1000, 1000));
    }

    #[test]
    fn release_writes_the_true_original_into_rc_normal() {
        // The (0,48) inset is real and current: probed on this machine
        // 2026-08-26 with EnumDisplayMonitors + GetMonitorInfoW, the primary
        // reports rcMonitor (0,0)-(2560,1600) and rcWork (0,48)-(2560,1600),
        // i.e. a bar docked across the top eating 48 physical pixels.
        //
        // WHAT IS NOT MEASURED, and an earlier version of this comment said
        // was: no real window has been watched through GetWindowPlacement on
        // this machine, so the round trip "screen (320,232)-(2240,1367) reads
        // back (320,184)-(2240,1319)" is arithmetic, not an observation. The
        // rect below is a worked example of the inset, nothing more. Settling
        // whether the write-back lands correctly still needs the hand test:
        // PiP a maximised window, maximise it, wait for the release toast,
        // un-maximise, and check it returns to where it started.
        assert_eq!(
            rc_normal_for_release(320, 232, 1920, 1135, 0, 48),
            (320, 184, 2240, 1319)
        );
        // Stock layout (bottom taskbar): inset (0,0) — the identity. This is
        // the configuration where a MISSING conversion silently passes.
        assert_eq!(
            rc_normal_for_release(100, 100, 640, 480, 0, 0),
            (100, 100, 740, 580)
        );
        // A left-docked bar insets horizontally instead.
        assert_eq!(
            rc_normal_for_release(500, 300, 800, 600, 64, 0),
            (436, 300, 1236, 900)
        );
    }

    // ── §7 disposition — which release, and whether the entry survives it.
    // TASK 3b's whole point is that the cache entry can be the LAST copy of
    // the pre-PiP bounds, so "delete it anyway" is unrecoverable data loss.

    #[test]
    fn a_maximized_window_gets_the_full_release() {
        // rcNormalPosition is not displaying anything, so it can be rewritten
        // invisibly and the entry is then redundant.
        assert_eq!(
            release_disposition(true, PipState::Active, false),
            Some(Disposition::Full)
        );
        // Even if topmost was already dropped by an earlier half-release —
        // maximising later is exactly the chance to finish the job.
        assert_eq!(
            release_disposition(true, PipState::Released, false),
            Some(Disposition::Full)
        );
    }

    #[test]
    fn true_fullscreen_keeps_the_entry() {
        // F11 in any Chromium app: SC_RESTORE then size-to-monitor, so
        // IsZoomed is FALSE while the window covers everything. Its normal
        // rect is live geometry — writing the original bounds there would
        // yank it out of fullscreen — so topmost is dropped and the entry is
        // KEPT. Deleting it here is the loss TASK 3b forbids, reached by F11
        // instead of the maximise button.
        assert_eq!(
            release_disposition(false, PipState::Active, false),
            Some(Disposition::HalfKeepEntry)
        );
    }

    #[test]
    fn a_half_released_window_is_not_re_processed_every_tick() {
        // This runs twice a second for as long as the window stays
        // fullscreen; without this arm it would re-drop topmost and re-toast
        // on every one of them.
        assert_eq!(release_disposition(false, PipState::Released, false), None);
    }

    // ── §7 claim — the watcher decides on a snapshot and acts later.

    #[test]
    fn the_claim_takes_the_entry_it_actually_judged() {
        let mut map: HashMap<PipKey, PipEntry> = HashMap::new();
        map.insert(ck(0xA), entry(10, 20, 30, 40, 5));
        let got = claim_for_release(&mut map, ck(0xA), 5, Disposition::Full);
        assert_eq!(got.map(|e| e.original_x), Some(10));
        assert!(map.is_empty(), "a full release must forget the window");
    }

    #[test]
    fn the_claim_refuses_an_entry_created_after_the_snapshot() {
        // The engine restored this window (5th tap, entry gone) and the user
        // immediately re-entered PiP (new entry, same HWND). A bare
        // `remove(&key)` returns Some here and the watcher releases a PiP that
        // is milliseconds old — inside the settle grace, which was checked
        // against the OLD entry and is never re-checked on the claimed one.
        let mut map: HashMap<PipKey, PipEntry> = HashMap::new();
        map.insert(ck(0xA), entry(10, 20, 30, 40, 6)); // serial 6: the NEW entry
        let got = claim_for_release(&mut map, ck(0xA), 5, Disposition::Full);
        assert!(got.is_none(), "a stale snapshot must not claim a new entry");
        assert!(
            map.contains_key(&ck(0xA)),
            "and it must leave the new entry intact — it is a live PiP"
        );
    }

    #[test]
    fn a_half_release_claim_flags_the_entry_instead_of_removing_it() {
        let mut map: HashMap<PipKey, PipEntry> = HashMap::new();
        map.insert(ck(0xA), entry(10, 20, 30, 40, 5));
        let got = claim_for_release(&mut map, ck(0xA), 5, Disposition::HalfKeepEntry);
        assert!(got.is_some());
        let kept = map
            .get(&ck(0xA))
            .expect("the entry is the last copy of the bounds — keep it");
        assert_eq!(
            kept.state,
            PipState::Released,
            "and mark it Released, so the next tick skips it AND the next tap re-enters"
        );
        assert_eq!(kept.original_w, 30, "with the pre-PiP bounds untouched");
    }

    #[test]
    fn a_reentered_window_keeps_its_new_topmost() {
        // Between the claim and the HWND_NOTOPMOST the engine can enter PiP
        // again and set HWND_TOPMOST; ours would land last and leave a window
        // that is tracked as PiP, animating to a corner and captioned
        // "PiP: Top-Left", but not on top of anything.
        let mut map: HashMap<PipKey, PipEntry> = HashMap::new();
        assert!(
            !reentered_since_claim(&map, ck(0xA), 5),
            "an empty slot is the normal case — go ahead and drop topmost"
        );
        map.insert(ck(0xA), entry(0, 0, 1, 1, 9));
        assert!(reentered_since_claim(&map, ck(0xA), 5));
        map.insert(ck(0xA), entry(0, 0, 1, 1, 5));
        assert!(
            !reentered_since_claim(&map, ck(0xA), 5),
            "our own kept half-release entry is not a re-entry"
        );
    }

    // ── §8 / TASK 1 — a RELEASED entry is not an active PiP.
    //
    // The bug being pinned: a half-released window is no longer topmost, but
    // the engine could not tell, so the next tap CYCLED it to the next corner.
    // Only entry asserts HWND_TOPMOST — cycling never does — so the user got a
    // window hopping between corners from behind everything else.

    /// A released entry with the window's real pid, at some corner, as the
    /// watcher would have left it.
    fn released_entry(x: i32, y: i32, w: i32, h: i32, serial: u64, pid: u32) -> PipEntry {
        PipEntry {
            position_index: 2,
            pid,
            state: PipState::Released,
            ..entry(x, y, w, h, serial)
        }
    }

    #[test]
    fn an_active_entry_still_cycles_then_restores() {
        // The control. Without it, "released entries do not cycle" could pass
        // with cycling broken for everyone.
        let mut map: HashMap<PipKey, PipEntry> = HashMap::new();
        map.insert(ck(0xA), entry(10, 20, 300, 400, 1));
        assert_eq!(tap_for(&mut map, ck(0xA), 999), Tap::Cycle(1));
        assert_eq!(tap_for(&mut map, ck(0xA), 999), Tap::Cycle(2));
        assert_eq!(tap_for(&mut map, ck(0xA), 999), Tap::Cycle(3));
        // 5th tap: restore to the ORIGINAL frame and let go.
        match tap_for(&mut map, ck(0xA), 999) {
            Tap::Restore(e) => {
                assert_eq!((e.original_x, e.original_y, e.original_w, e.original_h), (10, 20, 300, 400))
            }
            other => panic!("the 5th tap must restore, got {other:?}"),
        }
        assert!(map.is_empty(), "and the entry is gone afterwards");
    }

    #[test]
    fn a_released_entry_is_not_cycled() {
        let mut map: HashMap<PipKey, PipEntry> = HashMap::new();
        map.insert(ck(0xA), released_entry(10, 20, 300, 400, 1, 4242));

        let tap = tap_for(&mut map, ck(0xA), 4242);
        match tap {
            Tap::Enter(Some(prev)) => {
                assert_eq!(prev.original_x, 10, "and it hands the preserved bounds on");
            }
            other => panic!(
                "a released entry must read as a FRESH ENTRY, not a corner cycle — got {other:?}"
            ),
        }
        // The bookkeeping must not have advanced either: a released entry that
        // came back as Enter but had its position_index bumped would send the
        // re-entry to corner 1 instead of corner 0 the moment anything read it.
        assert_eq!(
            map.get(&ck(0xA)).map(|e| e.position_index),
            Some(2),
            "tap_for must not touch a released entry's corner bookkeeping"
        );
    }

    #[test]
    fn re_entry_preserves_the_original_bounds_through_to_the_fifth_tap() {
        // THE WHOLE POINT OF §8, end to end, through the real functions.
        //
        // A window enters PiP from a 1600x900 frame at (120,80). It goes true
        // fullscreen; the watcher half-releases it (bounds kept, no write-back
        // possible). The user taps PiP again — and if that re-entry re-measured
        // the window it would store the FULLSCREEN rect as `original_*`, so the
        // 5th tap would "restore" the window to the size of the screen and the
        // real frame would be gone from the machine entirely.
        const ORIGINAL: (i32, i32, i32, i32) = (120, 80, 1600, 900);
        let mut map: HashMap<PipKey, PipEntry> = HashMap::new();
        map.insert(ck(0xA), entry(ORIGINAL.0, ORIGINAL.1, ORIGINAL.2, ORIGINAL.3, 7));
        if let Some(e) = map.get_mut(&ck(0xA)) {
            e.pid = 4242;
        }

        // F11 → not zoomed, area over the line → half-release.
        assert!(should_release(false, 1920, 1080, 1920, 1040));
        let disposition = release_disposition(false, PipState::Active, false)
            .expect("a fullscreen PiP window must be released");
        assert_eq!(disposition, Disposition::HalfKeepEntry);
        assert!(claim_for_release(&mut map, ck(0xA), 7, disposition).is_some());

        // What restore_all() would find right now if the user quit here: the
        // entry, still holding the true frame. That is why it is kept.
        assert_eq!(
            map.get(&ck(0xA)).map(|e| (e.original_x, e.original_y, e.original_w, e.original_h)),
            Some(ORIGINAL),
            "the released entry is the last copy of the pre-PiP bounds — restore_all reads it"
        );

        // Next tap: fresh entry, carrying those bounds.
        let Tap::Enter(Some(preserved)) = tap_for(&mut map, ck(0xA), 4242) else {
            panic!("a released entry must re-enter, not cycle")
        };

        // The entry arm publishes a NEW entry from `preserved` — new serial,
        // corner 0, Active — and copies the bounds VERBATIM. (That the arm
        // really does copy rather than measure is asserted separately, from
        // the source, by the test below; a foreground window is needed to run
        // it for real.)
        map.insert(
            ck(0xA),
            PipEntry {
                position_index: 0,
                serial: 8,
                state: PipState::Active,
                ..preserved
            },
        );

        // Four more taps, exactly as the user would: 3 corners then restore.
        assert_eq!(tap_for(&mut map, ck(0xA), 4242), Tap::Cycle(1));
        assert_eq!(tap_for(&mut map, ck(0xA), 4242), Tap::Cycle(2));
        assert_eq!(tap_for(&mut map, ck(0xA), 4242), Tap::Cycle(3));
        match tap_for(&mut map, ck(0xA), 4242) {
            Tap::Restore(e) => assert_eq!(
                (e.original_x, e.original_y, e.original_w, e.original_h),
                ORIGINAL,
                "the 5th tap after a re-entry MUST restore the pre-PiP frame, not the \
                 fullscreen rect the window was showing when PiP was re-entered"
            ),
            other => panic!("expected the 5th tap to restore, got {other:?}"),
        }
    }

    /// The source-level half of the guarantee above.
    ///
    /// `tap_for` handing the preserved entry back only matters if the ENTRY ARM
    /// then STORES those numbers instead of measuring the window — and no
    /// value-level test can see which of the two the arm chose, because that
    /// code lives in `toggle_pip_win32`, which needs a real foreground window
    /// to run at all. So this asserts the structural property directly, the
    /// same way `browser_profiles.rs` asserts that its dispatcher consults the
    /// hard-requirement guard.
    #[test]
    fn the_entry_arm_reuses_preserved_bounds_and_measures_only_without_them() {
        let src = include_str!("pip.rs").replace("\r\n", "\n");
        let anchor = "let (ox, oy, ow, oh, maximized) = if let Some(prev) = &preserved {";
        let start = src
            .find(anchor)
            .expect("the entry arm must still branch on `preserved`");
        let end = src[start..]
            .find("let t_after_read")
            .expect("the measurement block must still end at t_after_read");
        let block = &src[start..start + end];

        let split = block
            .find("} else {")
            .expect("the entry arm must still have both branches");
        let (reuse_branch, measure_branch) = block.split_at(split);

        assert!(
            reuse_branch.contains("prev.original_x")
                && reuse_branch.contains("prev.original_y")
                && reuse_branch.contains("prev.original_w")
                && reuse_branch.contains("prev.original_h")
                && reuse_branch.contains("prev.was_maximized"),
            "the re-entry branch must reuse ALL FOUR preserved bounds and the show state — \
             anything it recomputes instead is geometry PiP itself created"
        );
        assert!(
            !reuse_branch.contains("measure_original_frame"),
            "the re-entry branch must NOT measure the window: it is showing the fullscreen \
             rect (or PiP's own corner tile) right now, and storing that as original_* \
             destroys the last copy of the real frame — the exact data loss §7/§8 exist to \
             prevent"
        );
        assert!(
            measure_branch.contains("measure_original_frame("),
            "and a genuine first entry, with nothing preserved, must still measure"
        );
    }

    #[test]
    fn a_recycled_handle_does_not_re_enter_on_a_strangers_window() {
        // NATIVE_SAFETY rule 3. A released entry can sit for hours; if Windows
        // hands its HWND to another process's window, reusing the stored bounds
        // would tile a stranger's window and later "restore" it to a frame it
        // never had. Measuring the window that is actually there is the only
        // honest answer.
        let mut map: HashMap<PipKey, PipEntry> = HashMap::new();
        map.insert(ck(0xA), released_entry(10, 20, 300, 400, 1, 4242));
        assert_eq!(
            tap_for(&mut map, ck(0xA), 777),
            Tap::Enter(None),
            "a pid mismatch must fall back to a measured entry, not reuse stale bounds"
        );
        assert!(map.is_empty(), "and the stale entry must be dropped");
    }

    #[test]
    fn an_unknowable_pid_keeps_the_bounds_rather_than_destroying_them() {
        // The other half of the rule above. A zero pid on either side means
        // "could not tell", and treating that as "different" would throw away
        // the last copy of the pre-PiP bounds on the strength of a failed
        // query. Only a POSITIVE disagreement between two known pids is
        // evidence of anything.
        let mut map: HashMap<PipKey, PipEntry> = HashMap::new();
        map.insert(ck(0xA), released_entry(10, 20, 300, 400, 1, 0));
        assert_eq!(
            tap_for(&mut map, ck(0xA), 777),
            Tap::Enter(Some(released_entry(10, 20, 300, 400, 1, 0)))
        );

        let mut map: HashMap<PipKey, PipEntry> = HashMap::new();
        map.insert(ck(0xA), released_entry(10, 20, 300, 400, 1, 4242));
        match tap_for(&mut map, ck(0xA), 0) {
            Tap::Enter(Some(e)) => assert_eq!(e.original_w, 300),
            other => panic!("an unreadable current pid must not destroy bounds — got {other:?}"),
        }
    }

    #[test]
    fn a_released_entry_is_not_released_again_on_the_next_tick() {
        // The watcher runs twice a second. Without the Released arm this would
        // re-claim, re-drop topmost and re-toast on every tick for as long as
        // the window stayed fullscreen.
        let mut map: HashMap<PipKey, PipEntry> = HashMap::new();
        map.insert(ck(0xA), entry(10, 20, 300, 400, 5));

        let first = release_disposition(false, PipState::Active, false).expect("tick 1 releases");
        assert!(claim_for_release(&mut map, ck(0xA), 5, first).is_some());

        // Ticks 2..n: same window, same non-zoomed fullscreen state.
        let state = map.get(&ck(0xA)).expect("kept").state;
        assert_eq!(
            release_disposition(false, state, false),
            None,
            "nothing left to do — no second topmost drop, no second toast"
        );
    }

    // ─────────────────────────────────────────────────────────────────────
    // §9 — Space+Tab, fullscreen-preserving PiP.
    //
    // Every number below that describes a real window came off the
    // 2026-08-29 measurement against a throwaway Brave: a genuinely
    // fullscreen window reported style 0x160B0000, IsZoomed false, covering
    // rcMonitor (0,0)-(2560,1600) — and at one sample 2560x1599, one pixel
    // short, which is why the slack exists. The ordinary windowed control
    // reported 0x16CF0000. The two differ in exactly WS_CAPTION|WS_THICKFRAME.
    // ─────────────────────────────────────────────────────────────────────

    /// The measured styles, named so the tests read as the observation they
    /// are rather than as magic constants.
    const STYLE_FULLSCREEN: u32 = 0x160B_0000;
    const STYLE_WINDOWED: u32 = 0x16CF_0000;
    /// The measured ex-style, identical in both states — captured anyway so a
    /// future app that DOES move an ex-style bit is covered.
    const EXSTYLE_FULLSCREEN: u32 = 0x0020_0000;
    /// What today's broken restore produced: `STYLE_FULLSCREEN | WS_MAXIMIZE`.
    /// Measured 2026-08-29 by replaying the restore against a throwaway Brave,
    /// and identical to the style in the owner's own failing log.
    const STYLE_MAXIMIZED_FULLSCREEN: u32 = 0x170B_0000;
    const MON: (i32, i32, i32, i32) = (0, 0, 2560, 1600);

    #[test]
    fn a_measured_fullscreen_window_reads_as_fullscreen() {
        assert!(is_fullscreen_geometry(
            STYLE_FULLSCREEN,
            false,
            (0, 0, 2560, 1600),
            MON
        ));
    }

    #[test]
    fn one_pixel_short_of_the_monitor_is_still_fullscreen() {
        // NOT hypothetical: the probe caught the real window reporting
        // 2560x1599 on a 2560x1600 monitor. An exact-equality test would have
        // sent the owner down the fallback path on the exact case this
        // feature exists for.
        assert!(is_fullscreen_geometry(
            STYLE_FULLSCREEN,
            false,
            (0, 0, 2560, 1599),
            MON
        ));
    }

    #[test]
    fn an_ordinary_window_is_not_fullscreen_however_big_it_is() {
        // The style half doing the work: this window covers the whole
        // monitor and still has its caption and resize frame, so it is a
        // window someone dragged large — not a fullscreen one. Preserving a
        // fullscreen state it does not have would mean placing suppressed-
        // veto moves on a window that never asked for them.
        assert!(!is_fullscreen_geometry(
            STYLE_WINDOWED,
            false,
            (0, 0, 2560, 1600),
            MON
        ));
    }

    #[test]
    fn a_maximised_window_is_not_fullscreen() {
        // Space+` already handles maximised windows, and handles them by
        // un-maximising first. If this said yes, a maximised window would get
        // the fullscreen path — which never un-maximises — and produce §6's
        // "corner tile that is secretly still a zoomed window".
        assert!(!is_fullscreen_geometry(
            STYLE_FULLSCREEN,
            true,
            (0, 0, 2560, 1600),
            MON
        ));
    }

    #[test]
    fn a_chromeless_window_that_does_not_cover_its_screen_is_not_fullscreen() {
        // The geometry half. A borderless splash screen, or — the one that
        // matters — a window ALREADY sitting in a fullscreen-PiP corner tile.
        assert!(!is_fullscreen_geometry(
            STYLE_FULLSCREEN,
            false,
            (0, 0, 1280, 800),
            MON
        ));
    }

    #[test]
    fn a_degenerate_monitor_rect_fails_toward_not_fullscreen() {
        // A failed/empty monitor read would make "covers it" trivially true
        // for every window on the machine. Fail toward the fallback, which is
        // today's behaviour and is never harmful.
        assert!(!is_fullscreen_geometry(
            STYLE_FULLSCREEN,
            false,
            (0, 0, 2560, 1600),
            (0, 0, 0, 0)
        ));
    }

    #[test]
    fn the_corner_tiles_for_a_fullscreen_source_are_half_the_work_area() {
        // The corner maths for a FULLSCREEN source, which is the case
        // `corners_are_the_four_corners_of_the_work_area` cannot cover: the
        // source rect is rcMONITOR (2560x1600, taskbar included) while the
        // tiles are measured against rcWORK. With a 48px bar docked at the
        // top — the layout probed on this machine 2026-08-26 — the two
        // differ, and a tile computed from the monitor instead of the work
        // area would sit under that bar.
        let (l, t, w, h) = (0, 48, 2560, 1552); // work area, top bar
        let (pw, ph) = (w / 2, h / 2); // 1280x776
        assert_eq!(corner_position(0, l, t, pw, ph, w, h), (0, 48));
        assert_eq!(corner_position(1, l, t, pw, ph, w, h), (1280, 48));
        assert_eq!(corner_position(2, l, t, pw, ph, w, h), (1280, 824));
        assert_eq!(corner_position(3, l, t, pw, ph, w, h), (0, 824));
        // And the tile is a quarter of the work area, not of the monitor.
        assert_eq!((pw, ph), (1280, 776));
    }

    #[test]
    fn only_a_fullscreen_entry_suppresses_the_veto() {
        use windows::Win32::UI::WindowsAndMessaging::{
            SWP_NOACTIVATE, SWP_NOSENDCHANGING, SWP_NOZORDER,
        };
        // THE measurement, as one assertion: without SWP_NOSENDCHANGING a
        // fullscreen Chromium window is back at 2560x1600 within 40ms.
        assert_eq!(
            move_flags(true),
            SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOSENDCHANGING
        );
        // And Space+` is untouched — byte-for-byte the flags `animate_to`
        // has always used.
        assert_eq!(move_flags(false), SWP_NOZORDER | SWP_NOACTIVATE);
    }

    #[test]
    fn the_verification_accepts_rounding_and_rejects_a_snap_back() {
        let target = (0, 48, 1280, 776);
        assert!(placement_held(target, (0, 48, 1280, 776)), "exact");
        assert!(
            placement_held(target, (1, 47, 1282, 774)),
            "a pixel or two of DPI rounding is not a failure"
        );
        // The failure this exists to catch: the window is back at full size.
        assert!(
            !placement_held(target, (0, 0, 2560, 1600)),
            "a window that reasserted its monitor bounds must be detected"
        );
        // And the subtler one: right size, wrong place.
        assert!(!placement_held(target, (640, 400, 1280, 776)));
    }

    #[test]
    fn a_fullscreen_corner_tile_does_not_trip_the_watcher() {
        // TRAP 2, settled by measurement rather than by exemption. The fear
        // was that a window kept in FULLSCREEN state would read as ≥75% of
        // the work area and be released the instant it was cornered. It does
        // not: after the move its REAL bounds are the tile — measured at
        // 1280x800 on a 2560x1600 work area — and IsZoomed is false, because
        // "fullscreen" here is the app's drawing state, not a window rect.
        assert!(!should_release(false, 1280, 800, 2560, 1600));
        // The control: the same window BEFORE the move, still covering the
        // screen, is correctly seen as enlarged.
        assert!(should_release(false, 2560, 1600, 2560, 1600));
    }

    #[test]
    fn a_fullscreen_entry_is_never_given_the_rc_normal_write_back() {
        // THE exemption that actually matters. A fullscreen entry's
        // `original_*` CAN be the monitor rect. `Disposition::Full` hands those
        // bounds to Windows as the window's rcNormalPosition, which would
        // record "this window's un-maximised size is the whole screen" —
        // a lie the user meets the next time they un-maximise, and one
        // nothing on the machine could then correct.
        assert_eq!(
            release_disposition(true, PipState::Active, true),
            Some(Disposition::HalfKeepEntry),
            "a fullscreen entry gets the HALF release even when zoomed: topmost dropped, \
             bounds kept, Windows told nothing false"
        );
        // The corner entry in the same state still gets the full release —
        // without this control the assertion above could pass with the
        // write-back broken for everyone.
        assert_eq!(
            release_disposition(true, PipState::Active, false),
            Some(Disposition::Full)
        );
        // And a fullscreen entry already half-released is not re-processed
        // twice a second, exactly like a corner one.
        assert_eq!(release_disposition(false, PipState::Released, true), None);
    }

    // ── THE RESTORE LEG (PROBLEM 219, amended 2026-08-29) ────────────────
    //
    // THE TELL WAS `zoomed=true`. Corner-cycling worked, all four corners
    // held chrome-less, and the 5th tap put the window back at exactly the
    // right bounds — so the geometry looked perfect and the feature was
    // still broken:
    //
    //   working corners: probe = true  (style 0x160B0000, zoomed=false)
    //   after 5th tap:   probe = false (style 0x170B0000, zoomed=true)
    //                                          ^^ WS_MAXIMIZE
    //
    // Same rect, different STATE. `restore_window` ended with
    // `ShowWindow(SW_SHOWMAXIMIZED)` whenever `was_maximized` was set — and
    // it IS set for a fullscreen Brave, because `GetWindowPlacement` reports
    // `showCmd == SW_SHOWMAXIMIZED` for a window that was maximised before it
    // went fullscreen. The window came back maximised instead of fullscreen,
    // so the next Space+Tab probed false and fell back to corner PiP, which
    // is what the owner saw as "the 5th fullscreen logic is broken".
    //
    // Generalise: WHEN A FEATURE PRESERVES A STATE, THE RESTORE MUST REPLAY
    // THAT STATE, NOT RE-DERIVE IT FROM GEOMETRY. Bounds equality is not
    // state equality, and a test that only checks bounds cannot see the
    // difference — which is precisely what the first cut's test did.

    #[test]
    fn the_fifth_tap_replays_the_captured_fullscreen_state_not_the_maximize_path() {
        // The whole bug, as one assertion. `was_maximized` is TRUE here
        // because that is what the owner's log recorded for his fullscreen
        // Brave — and the plan must still be the fullscreen one.
        let e = PipEntry {
            was_maximized: true,
            ..fs_entry(1, 49, 2558, 1550, 11)
        };
        match restore_plan(&e) {
            RestorePlan::Fullscreen(fs) => {
                assert_eq!(
                    (fs.style, fs.ex_style),
                    (STYLE_FULLSCREEN, EXSTYLE_FULLSCREEN),
                    "restore must reinstate the STYLE it captured — fullscreen is a style \
                     state, not a zoom state"
                );
                assert_eq!(
                    (fs.x, fs.y, fs.w, fs.h),
                    (0, 0, 2560, 1600),
                    "and the monitor rect it was taken from, NOT original_* — which for this \
                     window is its pre-fullscreen WINDOWED frame"
                );
                assert_ne!(
                    fs.style, STYLE_MAXIMIZED_FULLSCREEN,
                    "the reinstated style must not carry WS_MAXIMIZE: that is exactly the \
                     0x170B0000 the failing log showed"
                );
            }
            other => panic!(
                "a fullscreen entry must NEVER take the maximize path, even when \
                 was_maximized is true — got {other:?}"
            ),
        }
    }

    #[test]
    fn a_corner_entry_restores_exactly_as_it_always_did() {
        // The control, and the do-not-disturb guarantee for Space+`: an
        // ordinary entry still gets original_* plus the maximize path. Without
        // this the assertion above could pass with corner PiP's restore broken
        // for everyone.
        let plain = entry(10, 20, 300, 400, 1);
        assert_eq!(
            restore_plan(&plain),
            RestorePlan::Frame {
                x: 10,
                y: 20,
                w: 300,
                h: 400,
                maximize: false
            }
        );
        let was_max = PipEntry {
            was_maximized: true,
            ..entry(10, 20, 300, 400, 2)
        };
        assert_eq!(
            restore_plan(&was_max),
            RestorePlan::Frame {
                x: 10,
                y: 20,
                w: 300,
                h: 400,
                maximize: true
            },
            "a maximised corner PiP must still be re-maximised — that path is correct and \
             is not what PROBLEM 219's restore leg got wrong"
        );
    }

    #[test]
    fn a_fullscreen_entry_with_no_capture_falls_back_rather_than_inventing_one() {
        // Trap: the flag without the capture. It should be unreachable — they
        // are set and cleared together — but "unreachable" is the class of
        // branch PROBLEM 118 was about. The floor the owner named is that the
        // window is either fullscreen again or restored the way Space+` would
        // have restored it, never something in between.
        let e = PipEntry {
            fullscreen_state: None,
            was_maximized: true,
            ..fs_entry(1, 49, 2558, 1550, 12)
        };
        assert_eq!(
            restore_plan(&e),
            RestorePlan::Frame {
                x: 1,
                y: 49,
                w: 2558,
                h: 1550,
                maximize: true
            }
        );
    }

    #[test]
    fn a_demoted_entry_loses_its_fullscreen_state_with_its_flag() {
        // The fallback clears `fullscreen_pip` on a window that refused to
        // hold the tile. If the capture survived that, a later re-entry's
        // `.or_else` would carry it forward and hand `restore_plan` a
        // fullscreen to replay for a window that provably will not stay in one.
        let cache: PipCache = Arc::new(Mutex::new(HashMap::new()));
        cache.lock().unwrap().insert(ck(0xC), fs_entry(0, 0, 2560, 1600, 30));
        clear_fullscreen_flag(&cache, ck(0xC), 30);
        let e = cache.lock().unwrap().get(&ck(0xC)).cloned().expect("kept");
        assert!(!e.fullscreen_pip);
        assert!(
            e.fullscreen_state.is_none(),
            "the capture must go with the flag, or a demoted entry can still be restored as \
             a fullscreen one"
        );
        cache.lock().unwrap().clear();
    }

    #[test]
    fn the_fifth_tap_still_hands_back_the_flagged_entry_and_lets_go() {
        // The cache round trip, unchanged by the restore-leg work: four taps
        // cycle, the fifth restores and PiP forgets the window. This is what
        // makes `restore_plan` reachable at all.
        let mut map: HashMap<PipKey, PipEntry> = HashMap::new();
        map.insert(ck(0xA), fs_entry(1, 49, 2558, 1550, 11));
        assert_eq!(tap_for(&mut map, ck(0xA), 4242), Tap::Cycle(1));
        assert_eq!(tap_for(&mut map, ck(0xA), 4242), Tap::Cycle(2));
        assert_eq!(tap_for(&mut map, ck(0xA), 4242), Tap::Cycle(3));
        match tap_for(&mut map, ck(0xA), 4242) {
            Tap::Restore(e) => {
                assert!(e.fullscreen_pip);
                assert!(
                    e.fullscreen_state.is_some(),
                    "the entry the 5th tap hands to restore_window must still carry the \
                     capture — it is the only record of what fullscreen looked like"
                );
            }
            other => panic!("the 5th tap must restore, got {other:?}"),
        }
        assert!(map.is_empty(), "and PiP lets go of the window afterwards");
    }

    #[test]
    fn the_entry_arm_stores_the_probes_capture_and_keeps_it_sticky() {
        // The structural half, for the same reason the sticky-flag test is
        // structural: this expression lives in `toggle_pip_win32`, which needs
        // a real foreground window to run. A capture that is taken and then
        // not stored would leave every entry falling back to the frame
        // restore — i.e. the bug, still there, with all the new code present.
        let src = include_str!("pip.rs").replace("\r\n", "\n");
        assert!(
            src.contains("probed_state = Some(fs);"),
            "the probe's capture must be taken on the yes branch"
        );
        assert!(
            src.contains(
                "fullscreen_state: probed_state\n                            .or_else(|| preserved.as_ref().and_then(|p| p.fullscreen_state)),"
            ),
            "and stored on the entry, preferring a fresh capture but never dropping a \
             preserved one — a released fullscreen entry re-entered by Space+` has no probe \
             of its own, and its stored capture is the last record of the real state"
        );
    }

    #[test]
    fn the_fallback_demotes_the_entry_it_was_spawned_for_and_no_other() {
        // The verification runs FS_VERIFY_MS after the placement, and in that
        // gap the user can have restored this window and entered PiP again.
        // Clearing the flag on that new entry would silently turn a working
        // fullscreen PiP into a corner one — the same class of stale-claim
        // bug `claim_for_release` closes on the watcher side, which is why it
        // is closed the same way.
        let cache: PipCache = Arc::new(Mutex::new(HashMap::new()));
        cache.lock().unwrap().insert(ck(0xB), fs_entry(0, 0, 2560, 1600, 20));

        clear_fullscreen_flag(&cache, ck(0xB), 20);
        assert!(
            !cache.lock().unwrap().get(&ck(0xB)).unwrap().fullscreen_pip,
            "the entry it was spawned for must be demoted to an ordinary corner PiP"
        );

        // A newer entry under the same key must be left alone.
        cache.lock().unwrap().insert(ck(0xB), fs_entry(0, 0, 2560, 1600, 21));
        clear_fullscreen_flag(&cache, ck(0xB), 20);
        assert!(
            cache.lock().unwrap().get(&ck(0xB)).unwrap().fullscreen_pip,
            "a stale flight must not demote a PiP the user created after it"
        );
        cache.lock().unwrap().clear();
    }

    #[test]
    fn a_reentry_over_fullscreen_bounds_keeps_the_flag_sticky() {
        // The structural guarantee behind the write-back exemption. Once
        // `original_*` holds fullscreen bounds, the entry that carries them
        // must stay marked no matter which key re-enters, or a later
        // `Disposition::Full` would hand the monitor rect to
        // rcNormalPosition. The entry arm composes the flag as
        // `effective == FullscreenPreserving || preserved.fullscreen_pip`;
        // this asserts the OR is still there, because no value-level test can
        // reach that expression — it lives in `toggle_pip_win32`, which needs
        // a real foreground window to run.
        let src = include_str!("pip.rs").replace("\r\n", "\n");
        assert!(
            src.contains(
                "fullscreen_pip: effective == PipMode::FullscreenPreserving\n                            || preserved.as_ref().is_some_and(|p| p.fullscreen_pip),"
            ),
            "the entry arm must OR the preserved flag in, not overwrite it: a released \
             fullscreen entry re-entered by Space+` still holds MONITOR-rect bounds, and \
             clearing the flag would let the watcher write them into rcNormalPosition"
        );
    }
}
