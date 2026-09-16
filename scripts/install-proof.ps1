# ---------------------------------------------------------------------------
# install-proof.ps1 — the evidence half of install-real.cmd.
#
# It is a separate FILE rather than a `powershell -Command` one-liner inside the
# .cmd because that one-liner has to survive two levels of quoting and a line
# continuation per line, and it did not: a path came out mangled and the check
# silently measured nothing. A script file has one level of quoting.
#
# Run it only through install-real.cmd, which explorer.exe launches from outside
# the agent's MSIX container (PROBLEM 143). Run from the agent shell it will
# read the container's copy and cheerfully agree with itself.
# ---------------------------------------------------------------------------
param(
  [Parameter(Mandatory = $true)][string]$Exe,
  [Parameter(Mandatory = $true)][string]$Root,
  [Parameter(Mandatory = $true)][string]$Out
)

# Start a FRESH report every run. This used to open with Add-Content, so a run
# that died part-way (a usage limit, on 2026-09-07) left its block in the file
# and the NEXT run appended below it — producing an output whose FIRST, most
# readable block was a complete, plausible, STALE proof naming the previous
# version. install-real.cmd deletes this file afterwards, which hid the bug for
# as long as every run finished. A report that can show yesterday's verdict at
# the top is not a report.
Set-Content $Out ''

$item = Get-Item $Exe
Add-Content $Out ("version: " + $item.VersionInfo.FileVersion)
Add-Content $Out ("written: " + $item.LastWriteTime)

$run = (Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' `
          -Name Spaceadom -ErrorAction SilentlyContinue).Spaceadom
Add-Content $Out ("Run key: " + $run)

# A RUST marker still works — those strings are not compressed.
# THREE POSITIVE CONTROLS, all shipped long before this release: 'rival
# install', 'hud-pointer: could not spawn', 'start_menu_scan:'. A False on
# those means the scan technique broke, not that a fix is missing.
# The last two are NEW in 1.0.88, and both halves of the pair were measured
# before this install ran (2026-08-27 10:29, scripts/preinstall-probe.ps1
# against the installed 1.0.87): controls True, '), dead zone ' False,
# 'hud_show_specials' False — and both confirmed PRESENT in the freshly-built
# target
elease\spaceadom.exe at 10:28 BEFORE the baseline was taken. Both
# halves are required: an unconfirmed marker's False means "never findable",
# not "did not ship" (see the short-literal note below).
#   '), dead zone '     is a piece of pointer.rs's chip-publish log FORMAT
#                        string. format_args! pieces are always &'static str in
#                        .rodata, so the immediate-store trap cannot reach them
#                        however short the piece is — which is exactly why a
#                        format string is the right shape for a marker.
$bytes = [Text.Encoding]::ASCII.GetString([IO.File]::ReadAllBytes($Exe))
#
# 2026-08-27, MEASURED: 'st-hud-pointer' is NOT usable as a marker even though
# it is a plain literal in pointer.rs. A short literal that is only ever copied
# into a String gets materialised by IMMEDIATE MOV INSTRUCTIONS instead of
# living in .rodata — at 0x2C8B3 the 14-byte thread name is built by two
# overlapping 8-byte stores ("-pointer" at +6, then "st-hud-p" at +0), so the
# bytes never appear contiguously and any ASCII scan reports False on a binary
# that plainly contains the feature. 'st-exclusion-watcher' (20 bytes) and
# 'st-hook-supervisor' (18) DO survive in .rodata — too long for the trick.
# GENERALISE: a marker must be a LONG string (a log format string is ideal),
# and must be confirmed present in the freshly-built exe BEFORE it is trusted
# as evidence. A short thread name is not a marker.
# 1.0.89: the first FIVE are now all CONTROLS (they shipped in 1.0.88 or
# earlier and were measured True in the installed 1.0.88 at 13:51 by
# scripts/preinstall-probe.ps1). The last THREE are NEW in 1.0.89 and were
# measured False in that same baseline, having first been confirmed PRESENT
# in the freshly-built 1.0.89 exe at 13:50. Both halves are required.
#   'hud-band-count-changed' / 'hud-layout-changed' are the two new global
#   emit topics; they are passed to `emit` BY REFERENCE, so the compiler
#   cannot materialise them with immediate stores and they must sit in
#   .rodata. 'hud_magnetic_layout' is the new serde field name.
# 1.0.90: the first FIVE are CONTROLS (shipped in 1.0.88/1.0.89, measured TRUE
# in the installed 1.0.89 by preinstall-probe.ps1 before this install ran). The
# last FOUR are NEW in 1.0.90 (PROBLEM 214/215) and were measured FALSE in that
# same baseline, having first been confirmed PRESENT in the freshly-built
# 1.0.90 exe (ver 1.0.90, 18,994,176 bytes) at 20:11 on 2026-08-28. Both halves
# are required. Every new marker is a piece of a log FORMAT string, so
# format_args! puts it in .rodata and the short-literal trap cannot apply.
# 1.0.93's set. The first FIVE are CONTROLS - all measured True in the
# installed 1.0.92 (install-check.txt, 2026-08-29 02:30). The last TWO are NEW
# in 1.0.93 (the fullscreen-PiP restore leg) and were confirmed PRESENT in the
# freshly-built 1.0.93 exe (ver 1.0.93, 19,080,704 bytes) at 03:14 BEFORE the
# baseline probe ran, and measured False in that baseline. Both halves are
# required. Both are pieces of log FORMAT strings, so format_args! puts them in
# .rodata and the short-literal immediate-store trap cannot reach them.
# 1.0.95's set (2026-08-31, the email/account-label redesign). The first SEVEN
# are CONTROLS - every one shipped in 1.0.88..1.0.93 and is expected TRUE
# against the installed 1.0.94; a False across all of them means the scan
# technique broke, not that a fix is missing. The last TWO are NEW in 1.0.95:
# both are pieces of the SAME log FORMAT string, the one line that reports the
# account-label pass in browser_profiles::list_browser_profiles. format_args!
# pieces are always &'static str in .rodata, so the short-literal
# immediate-store trap cannot reach them however the compiler feels. Both were
# confirmed PRESENT in the freshly-built 1.0.95 exe BEFORE this baseline ran -
# which is what makes a False here evidence rather than an unmeasured guess.
#   'signed in and are labelled by the local part of their account'
#   'keep the browser''s own display name (no address is ever logged)'
# 1.0.96's set (2026-09-04). The first FIVE are CONTROLS - every one shipped in
# 1.0.88..1.0.95 and was measured TRUE in the installed 1.0.95 by
# scripts/preinstall-probe.ps1 before this install ran (baseline 03:33, ver
# 1.0.95, 19,144,704 bytes). A False across all five means the scan technique
# broke, not that a fix is missing. The last FOUR are NEW in 1.0.96 and were
# measured FALSE in that same baseline, having first been confirmed PRESENT in
# the freshly-built 1.0.96 exe under target\release BEFORE the baseline was
# read. Both halves are required: an unconfirmed marker's False means "never
# findable", not "did not ship".
# Every new marker is a whole log literal or a piece of a log FORMAT string, so
# format_args! puts it in .rodata and the short-literal immediate-store trap
# cannot reach it. All four are pure ASCII on purpose - the scan decodes the
# file as ASCII, so a marker containing an em-dash could never match.
#   'not been re-checked since the occlusion fix (PROBLEM 171). Re-running'
#                     - commands::retest_software_overlay_once, the one-shot.
#   ' profile(s) exist. Nothing was reordered.'
#                     - commands::apply_profile_reorder's count mismatch.
#   'run_overlay_fix: the check could not be run ('
#                     - commands::run_overlay_fix's error leg.
#   '. Nothing shown; the dashboard and the backend disagree about what the'
#                     - commands::preview_hud_layout's unknown-layout warn.
# 1.0.97's set (2026-09-04, evening). The first FIVE are the KEPT CONTROLS —
# every one shipped in 1.0.88..1.0.95 and every one was measured TRUE in the
# installed 1.0.96 by scripts/preinstall-probe.ps1 before this install ran
# (baseline 19:17:08, ver 1.0.96, 19,516,928 bytes). A False across all five
# means the scan technique broke, not that a fix is missing.
# The last FOUR are NEW in 1.0.97 and were measured FALSE in that same baseline,
# having first been confirmed PRESENT in the freshly-built 1.0.97 exe under
# target\release BEFORE the installer ran. Both halves are required: an
# unconfirmed marker's False means "never findable", not "did not ship".
# Every new marker is a piece of a log FORMAT string, so format_args! puts it in
# .rodata and the short-literal immediate-store trap cannot reach it. All four
# are pure ASCII and apostrophe-free on purpose.
#   'picker_worker: st-picker-scan started (os thread '
#                     - picker_worker.rs, PROBLEM 237's off-main-thread scan.
#   'hook: WATCHDOG would have alarmed ('
#                     - hook/mod.rs, PROBLEM 236's shadow-verdict line.
#   'rival install: REFUSING the elevated removal for '
#                     - rival_install.rs, the removal_target hard guard.
#   'no own-window check anywhere on this path (PROBLEM 243): not in the hook'
#                     - guide_hud/mod_impl.rs, the shown-over line.
# RETIRED, and why: the four 1.0.96 markers (the PROBLEM 171 one-shot, the
# reorder count mismatch, run_overlay_fix's error leg, preview_hud_layout's
# unknown-layout warn) all still exist in source and are now simply older
# controls; they were rotated out to keep the list at five controls + four new,
# not because they went stale. Nothing was retired for absence this release —
# every string on the previous list was re-grepped against src-tauri/src first.
# ONE near-miss worth recording: ' profile(s) exist. Nothing was reordered.'
# greps as ABSENT from the source because a `\` line-continuation splits it
# across two lines, while the compiled string is contiguous. A source grep is
# not a substitute for reading the literal.
# AND the reason 'guide_hud: shown over own window' is NOT the marker: that
# sentence is ASSEMBLED at runtime from a format piece plus shown_over_phrase()'s
# return, so it never sits contiguously on disk. Only the FORMAT piece scans.
# 1.0.98's set (2026-09-04, night — PROBLEM 244). SIX CONTROLS: five shipped in
# 1.0.88..1.0.96, and 'rival install: REFUSING the elevated removal for '
# shipped in 1.0.97, so a True on it also names the build the baseline was
# taken from. The last TWO are NEW in 1.0.98 and were measured FALSE in the
# installed 1.0.97 by scripts/preinstall-probe.ps1 before this install ran,
# having first been confirmed PRESENT in the freshly-built 1.0.98 exe under
# target\release. Both halves are required.
# Both new markers are pieces of log FORMAT strings, pure ASCII, no
# apostrophes:
#   'rival install: REGISTRY-ONLY removal (PROBLEM 244) - deleting the leftover'
#   'rival install: REFUSING msiexec /X for this product (PROBLEM 244) - its'
# RETIRED this release, and why: the three 1.0.97 markers about the picker
# worker, the shadow watchdog verdict and the PROBLEM 243 own-window line all
# still exist in source and are simply older; the picker one is KEPT as a
# control. Nothing was retired for absence.
# 1.0.99's set (2026-09-04, late — PROBLEM 245, the in-app updater). EIGHT
# CONTROLS: the whole 1.0.98 list, every one measured TRUE in the installed
# 1.0.98 before this install ran. The last FOUR are NEW in 1.0.99 and were
# measured FALSE in that same baseline, having first been confirmed PRESENT
# in the freshly-built 1.0.99 exe under target\release. Both halves are
# required. All four are pieces of updater.rs log FORMAT strings, chosen to
# contain no em dash (the ASCII scan turns one into '???' and the piece
# after it would not match):
#   'updater: install kind decided'                          - the NSIS/MSI/Unknown verdict line
#   ' for a release newer than '                             - the 'checking <url>' line
#   'escape hatch, PROBLEM 245)'                             - the auto_update=false skip line
#   'installing SILENTLY now (setup.exe /S /UPDATE /R /ARGS' - the line logged right before the plugin exits
# 1.0.101's set (2026-09-05). EIGHT CONTROLS: every one shipped in
# 1.0.88..1.0.99 and every one is expected TRUE against the installed 1.0.100.
# A False across all eight means the scan technique broke, not that a fix is
# missing; 'updater: install kind decided' and 'installing SILENTLY now
# (setup.exe /S /UPDATE /R /ARGS' shipped in 1.0.99, so a True on those two
# also names WHICH build this baseline is. Every control was re-grepped
# against src-tauri/src before this list was written - none is stale.
# The last FOUR are NEW in 1.0.101 and must read False here. Each is a piece
# of a log FORMAT string (format_args! puts those in .rodata, so the
# short-literal immediate-store trap cannot reach them), each pure ASCII and
# apostrophe-free, and each stops before the em dash in its sentence because
# the ASCII scan cannot match a multi-byte character:
#   'theme: watching the Windows app light/dark setting every '
#       - theme_watch.rs, the OS light/dark watcher REVIEW FIXES 2026-09-05
#         added and nothing has ever proved on the real machine.
#   'rival install: this PACKAGED copy found an unpackaged per-user install
#    beside it (PROBLEM 250 follow-up '
#       - rival_install::detect_cross_kind, FINDING C's Path 3.
#   'safe-mode: this process is the surviving single instance '
#       - safe_mode::note_surviving_instance, the boot counter moved out of
#         run() so a duplicate launch stops counting as a crash (C1).
#   'the one-time config snapshot would copy config.json and the backups into
#    a subfolder'
#       - packaged::migrate_legacy_data_once, the new SameDirectory verdict.
# 1.0.103's set (2026-09-06 - PROBLEM 257, the keyboard-deaf re-hook). TWELVE
# CONTROLS: the whole 1.0.101 list, every one of which shipped in
# 1.0.88..1.0.101 and is expected TRUE against the installed 1.0.102. 1.0.102
# added NO new Rust marker (its two additions were the frontend --ind-x /
# --ind-w pair), so the control list is carried forward unchanged and every
# string on it was re-grepped against src-tauri/src before this list was
# written - none is stale. A False across all twelve means the scan technique
# broke, not that a fix is missing.
# The last TWO are NEW in 1.0.103 and must read False against the installed
# 1.0.102. Both are pieces of log FORMAT strings (format_args! puts those in
# .rodata, so the short-literal immediate-store trap cannot reach them), both
# pure ASCII and apostrophe-free, and both deliberately START AFTER the em dash
# in their sentence because the ASCII scan cannot match a multi-byte character:
#   'Space is physically DOWN right now (GetAsyncKeyState, read on the
#    watchdog thread) with no hold latched'
#       - hook/mod.rs watchdog_check, the WARN half of
#         'hook: KEYBOARD DEAF, PROVEN (PROBLEM 257)'. The name of the law-6
#         detector itself; it cannot survive the detector being removed.
#   'the primary keyboard hook saw this Space-down and delivered it'
#       - engine/mod.rs, the SpaceDown arm's per-hold 'hold start (hold #N)'
#         line. This is the line install-proof.ps1's law-6 block greps for, so
#         a False here would mean the law-6 proof could never pass.
# 1.0.104's set (2026-09-07 - PROBLEM 259, the own-window keyboard FALLBACK).
# FOURTEEN CONTROLS: the whole 1.0.103 list - the twelve carried from 1.0.101
# plus 1.0.103's own two, which are controls now because 1.0.103 shipped and is
# the installed build this release is measured against. Every one was
# re-grepped against src-tauri/src before this list was written; none is stale.
# A False across all fourteen means the scan technique broke, not that a fix is
# missing.
# The last THREE are NEW in 1.0.104 and must read False against the installed
# 1.0.103. All three are pieces of log FORMAT strings (format_args! puts those
# in .rodata, so the short-literal immediate-store trap cannot reach them) and
# all three are pure ASCII, each deliberately stopping BEFORE the em dash in
# its sentence because the ASCII scan cannot match a multi-byte character:
#   'own-window fallback: Space-down came from the dashboard page (PROBLEM 257
#    fallback), hook silent'
#       - engine/mod.rs, the FALLBACK arm's per-hold line. This is the sentence
#         CLAUDE.md law 6's second table row names, and the whole point of
#         PROBLEM 259: it says the ring fired without the hook. It cannot
#         survive the fallback being removed.
#   'The keyboard hook did NOT see this Space; the page did, and'
#       - the same log line's SECOND format piece (the text after {phrase}).
#         Two pieces of one format string are two separate runs of .rodata, so
#         a True on both is evidence the whole literal shipped, not just its
#         head.
#   "PROVEN' and 'own-window fallback:'."
#       - engine/mod.rs, the OTHER arm: the hook's own 'hold start (hold #N)'
#         line now ends by telling the reader to grep for the fallback too.
#         In 1.0.103 that sentence ended at 'KEYBOARD DEAF, PROVEN'. It is on
#         this list so the evidence covers BOTH arms of the new branch - a
#         build where only the fallback arm was compiled is not a thing, but a
#         marker list that only watches one arm cannot say so.
# 1.0.105's set (2026-09-07 - PROBLEM 260, the forced repair that bypasses the
# cooldown). SEVENTEEN CONTROLS: the whole 1.0.104 list - the fourteen carried
# from 1.0.103 plus 1.0.104's own three, which are controls now because 1.0.104
# shipped and is the installed build this release is measured against. Every one
# was re-grepped against src-tauri/src before this list was written; none is
# stale. A False across all seventeen means the scan technique broke, not that a
# fix is missing.
# The last THREE are NEW in 1.0.105 and must read False against the installed
# 1.0.104. All three live in log FORMAT strings or in a const &str printed by
# one (format_args! pieces and a `const &str` are both &'static str in .rodata,
# so the short-literal immediate-store trap cannot reach them), and all three
# are pure ASCII, each deliberately chosen to avoid the em dash in its sentence
# because the ASCII scan cannot match a multi-byte character:
#   'MECHANISM (PROBLEM 260): when a WH_KEYBOARD_LL callback overruns
#    LowLevelHooksTimeout'
#       - hook/mod.rs TIMEOUT_EVICTION_DESC, the const that names the mechanism
#         (a timed-out WH_KEYBOARD_LL keeps a valid handle and is never called
#         again, while WH_MOUSE_LL keeps firing from its own timeout record).
#         It is printed on every forced repair and on the backoff line, so it
#         cannot survive PROBLEM 260's detector being removed.
#   "the old line read a 60s counter and printed '#1' three times). Handles:
#    keyboard "
#       - hook/mod.rs, the middle piece of the FORCED REPAIR warn: the sentence
#         that fixes the drained-counter lie AND the head of the old -> new
#         HHOOK triple. The handles are the only evidence a repair actually
#         changed the chain, so this piece watches the load-bearing half.
#   ' consecutive forced repair(s) delivered no keyboard callback, so the
#    backoff is engaged and the next repair waits '
#       - hook/mod.rs, the piece between {streak} and {wait} in the BACKOFF
#         warn. It is the only line that exists when a proven-deaf verdict is
#         held off, so a build without the 5 s floor / 60 s cap cannot have it.
# RETIRED IN 1.0.105, AND IT IS THE ONLY RETIREMENT THIS RELEASE - control #13,
# 'Space is physically DOWN right now (GetAsyncKeyState, read on the watchdog
# thread) with no hold latched'. It was PROBLEM 257's WARN sentence. PROBLEM 260
# folded that detector into `keyboard_deaf_with_space_down` and gave the verdict
# ONE voice - 'hook: KEYBOARD DEAF, PROVEN (PROBLEM 260, was 257) - reason
# {reason:?}' - so the old sentence now survives only as a DOC COMMENT on
# `ProvenDeaf::SpaceHeld`, and doc comments do not ship. This was MEASURED both
# ways before the swap, which is what makes it a retirement rather than a
# failure: _probe.0.105\marker-precheck.txt, read through explorer.exe against
# the installed 1.0.104, has it TRUE there and the scan of the freshly built
# 1.0.105 exe has it FALSE. A control that is True in the old build and False in
# the new one is a string the release deleted, not a scan that broke.
# ITS REPLACEMENT keeps the count at seventeen and watches the same instrument:
#   'callback-only counters (PROBLEM 236): nothing but a hook proc can move them'
#       - the 'hook liveness split' WARN, the line PROBLEM 260's forced-repair
#         message tells the reader to read next. Measured TRUE in the installed
#         1.0.104 in that same precheck, so it is a control on the day it joins
#         the list rather than a guess. It starts after the em dash in its
#         sentence, like every other entry, because the ASCII scan cannot match
#         a multi-byte character.
# 1.0.106's set (2026-09-07 - PROBLEM 261, pointer activation on a fallback
# hold, plus PROBLEM 239's second pass on the Conflicts cards). TWENTY
# CONTROLS: the whole 1.0.105 list - the seventeen carried from 1.0.104 plus
# 1.0.105's own three, which are controls now because 1.0.105 shipped and is
# the installed build this release is measured against. Every one of the twenty
# was MEASURED True in the installed 1.0.105 exe, read through explorer.exe,
# before this list was written: _probe\1.0.106\marker-precheck.txt. NOTHING WAS
# RETIRED this release - 1.0.105 retired control #13 and its replacement is
# still True; PROBLEM 261 deleted no log string, it only added.
# The last FOUR are NEW in 1.0.106 and were measured False against the
# installed 1.0.105 in that same precheck. All four are `format_args!` literal
# pieces of `log::` calls (so they are &'static str in .rodata, not the
# short-identifier immediate-store trap CLAUDE.md records), and all four are
# pure ASCII - each stops short of the em dash in its sentence, because the
# ASCII scan cannot match a multi-byte character:
#   'own-window fallback: the re-hook tore down a live fallback hold
#    (PROBLEM 261) '
#       - hook/mod.rs, the watchdog re-hook's teardown of a LIVE fallback hold.
#         This is the line that proves the new OWN_HOLD_ACTIVE latch is wired
#         into the repair path: a re-hook hides the ring, and before PROBLEM 261
#         nothing cleared the fallback's latch when it did, so the mouse
#         callback would have kept arming chips against a snapshot nobody could
#         see.
#   'own-window fallback: reaping a fallback Space-hold ('
#       - hook/mod.rs `reap_own_window_hold`, the HEAD of the fallback's own
#         stale-hold reaper - the foreground-probing twin of `reap_stale_hold`.
#   "Left standing this is a ring on screen with the pointer still arming chips
#    behind it, which is PROBLEM 218's failure on the path PROBLEM 218's reaper
#    cannot see"
#       - the TAIL of that same reaper literal, after the {hud_was_up} argument.
#         Head and tail are both on the list on purpose: a True on both is
#         evidence the whole literal shipped, not just its opening words. Same
#         reasoning 1.0.104 recorded for the two engine/mod.rs arms.
#   'own-window-holds-reaped(page stopped talking mid-hold):'
#       - the new field on the 60 s `hook diagnostics` line, beside
#         'stale-holds-reaped' and 'keyboard-deaf-rehooks'. It is the counter
#         `drain_own_holds_reaped` feeds, so a build without the fallback
#         reaper cannot print it.
# 1.0.107's set (2026-09-07 - PROBLEM 262, the stuck-hold latch). TWENTY-FOUR
# CONTROLS: the whole 1.0.106 list - the twenty carried from 1.0.105 plus
# 1.0.106's own four, which are controls now because 1.0.106 shipped and is the
# installed build this release is measured against. Every one of the twenty-four
# was re-grepped against src-tauri/src before this list was written and NONE is
# stale (24/24 still present in the source, with Rust's backslash-newline string
# continuations rejoined first - a marker split across two source lines is one
# run of .rodata but two runs of source text, and grepping the raw file reports
# a false stale). NOTHING WAS RETIRED this release: PROBLEM 262 deleted no log
# string, it only added.
# The last FOUR are NEW in 1.0.107 and must read False against the installed
# 1.0.106. All four are the LEADING piece of a `log::warn!` format string - the
# text before the first `{}` placeholder - so format_args! puts each one in
# .rodata as a single contiguous &'static str, out of reach of the
# short-identifier immediate-store trap CLAUDE.md records. All four are pure
# ASCII and each stops short of the em dash that follows it in its sentence,
# because the ASCII scan cannot match a multi-byte character. They are also, on
# purpose, long hyphenated sentences rather than words that could occur by
# accident anywhere else in the binary:
#   'stale-hold-reaped-because-the-keyboard-is-proven-deaf-spaceadom'
#       - hook/mod.rs, the HoldReap::KeyboardDeaf arm of `reap_stale_hold`.
#         The deafness-aware reap: a hold latched while the keyboard callback
#         has been silent past HOLD_DEAF_SILENCE_MS is a hold nothing on the
#         callback was ever going to end, and PROBLEM 219's combo stand-down
#         would have protected it forever. It feeds `deaf-holds-reaped` on the
#         60 s diagnostics line.
#   'modifier-active-latched-past-the-bound-with-no-keyboard-callbacks-spaceadom'
#       - hook/mod.rs, the HoldReap::LatchedPastBound arm. The last-resort
#         bound: MAX_MODIFIER_HOLD_MS enforced by the REAPER rather than by the
#         keyboard callback, which is the code that had stopped running. It
#         feeds `modifier-latch-bound-clears` on the same line.
#   'repair-tore-down-a-hold-that-predated-it-spaceadom'
#       - hook/mod.rs `tear_down_hold_across_repair`. A hold latched by a hook
#         chain that has just been replaced is unfalsifiable - its Space-UP
#         belongs to a hook that no longer exists - so the repair path now
#         tears it down instead of leaving MODIFIER_ACTIVE set, which is what
#         made guard 2 of the own-window fallback refuse every subsequent hold.
#         It feeds `repair-hold-teardowns`.
#   'deferral-episode-bound-expired-proceeding-with-the-repair-spaceadom'
#       - hook/mod.rs, the deferral bound measured from the FIRST alarm of an
#         episode. Before 1.0.107 the clock reset on every quiet tick, so with
#         the mouse moving the bound could never expire and this line could not
#         print at all.
# 1.0.108's set (2026-09-12 - PROBLEM 263 middle-button ring, PROBLEM 237
# follow-up picker stale-cache serve, PROBLEM 265 overlay early boot).
# TWENTY-EIGHT CONTROLS: the whole 1.0.107 list, 1.0.107's own four promoted
# to controls because 1.0.107 is the installed build this release is measured
# against. All 28 were re-grepped against src-tauri/src (continuations
# rejoined) before this list was written: 28/28 present, nothing retired.
# The last THREE are NEW in 1.0.108 and must read False against the installed
# 1.0.107. Each is the LEADING piece of a `log::info!` format string (the text
# before the first `{}`), so format_args! keeps it as one contiguous &'static
# str in .rodata. Each stops short of any em dash that follows it:
#   'middle-button ring: the-guide-hud-ring-was-raised-by-a-middle-mouse-button-hold-spaceadom'
#       - engine/mod.rs, the MiddleDown -> SpaceDown normalisation at the top
#         of `dispatch`. Printed once per middle-button hold. NEVER law 6's
#         proof (CLAUDE.md law 6: the keyboard hook was never asked anything).
#   'picker-serve-decision-path-and-list-age-marker-spaceadom-237:'
#       - picker_worker.rs `log_picker_served`, printed on every picker
#         answer with which path served it and how old the list was.
#   'overlay usable for the Guide HUD'
#       - lib.rs, the `overlay: configured` line now carries the ms since
#         process start (overlay_boot::since_start), PROBLEM 265.
# 1.0.109's set (2026-09-12 - PROBLEM 266, the logon task registered through
# the Task Scheduler COM API instead of schtasks.exe /Create). THIRTY-ONE
# CONTROLS: the whole 1.0.108 list, 1.0.108's own three promoted to controls
# because 1.0.108 is the installed build this release is measured against.
# The last ONE is NEW in 1.0.109 and must read False against the installed
# 1.0.108. It is the LEADING piece of a `log::info!` format string in
# startup.rs (the text before the em dash and the first `{}`), so format_args!
# keeps it as one contiguous &'static str in .rodata:
#   'startup: logon task registered for this user via the Task Scheduler API'
#       - startup.rs `ensure_startup_task`, printed once per launch when
#         Register-ScheduledTask (per-user AtLogOn trigger, Limited run level,
#         PT10S delay) succeeded; the HKCU Run value is removed right after.
#         Before 1.0.109 every non-admin install fell to the Run key because
#         `schtasks /Create /SC ONLOGON` writes an any-user trigger that only an
#         administrator may create (PROBLEM 64's real cause).
# 1.0.110's set (2026-09-13 - PROBLEM 267, the middle button's CURSOR-ANCHORED
# ICON RING, phase 2 of PROBLEM 263). THIRTY-TWO CONTROLS: the whole 1.0.109
# list, 1.0.109's own one promoted to a control because 1.0.109 is the
# installed build this LOCAL TEST build is measured against. The last ONE is
# NEW in 1.0.110 and must read False against the installed 1.0.109. It is the
# LEADING piece of a `log::info!` format string in guide_hud/mod_impl.rs
# `show_middle_ring` (the text before the em dash and the first `{}`), so
# format_args! keeps it as one contiguous &'static str in .rodata:
#   'middle-button ring v2: cursor-anchored-ring-raised-at-cursor-spaceadom-267'
#       - printed ONCE PER RAISE of the icon ring, i.e. only after the owner
#         has HELD THE MIDDLE BUTTON on this build. In the exe byte scan below
#         it is an install-time fact (the string shipped); in debug.log it is
#         an OWNER-TRIGGERED line, exactly like 1.0.108's
#         'middle-button ring: the-guide-hud-ring-was-raised-...' - its absence
#         from the log after an install is expected and is NOT a failure. It is
#         never law 6's proof either (CLAUDE.md law 6: the keyboard hook was
#         never asked anything by a mouse button).
# 1.0.111's set (2026-09-17 - PROBLEM 267 ROUND 6: the icon ring anchored on
# the press point, shrink-to-fit, guide ARCS, the scrim clipped to the room,
# move-then-size across mixed-DPI monitors; and Space + ; / the "Voice
# Typing" ring tile). THIRTY-THREE CONTROLS: the whole 1.0.110 list, 1.0.110's
# own one promoted to a control because 1.0.110 is the installed build this
# LOCAL TEST build is measured against. The last ONE is NEW in 1.0.111 and
# must read False against the installed 1.0.110. It is a plain `log::info!`
# literal (no format args) in engine/actions/voice_typing.rs:
#   'voice_typing: sent Win+H (Windows dictation)'
#       - printed once per Space + ; or per release on the ring's Voice
#         Typing tile. OWNER-TRIGGERED in debug.log (absence after an install
#         is expected); an install-time fact in the exe byte scan.
# 1.0.112's set (2026-09-17 - PROBLEM 268: the proven-deaf verdict reads a
# RAW-INPUT keyboard clock instead of inferring keystrokes from
# GetLastInputInfo minus the mouse callback — 449 false forced repairs in two
# days on this laptop's touchpad; and the icon-ring page rescales by the real
# devicePixelRatio). THIRTY-FOUR CONTROLS: the whole 1.0.111 list, 1.0.111's
# own one promoted to a control. The last ONE is NEW in 1.0.112 and must read
# False against the installed 1.0.111 — the LEADING piece of the
# `log::info!` in hook/mod.rs `register_raw_keyboard_sink`:
#   'hook: raw-input keyboard sink registered on the hook thread (hwnd '
#       - printed once per hook-thread start, i.e. at launch: an install-time
#         fact in the exe scan AND expected in debug.log after the banner.
foreach ($m in 'rival install', 'start_menu_scan:', 'hud-band-count-changed', 'restored to TRUE FULLSCREEN', 'picker_worker: st-picker-scan started (os thread ', 'rival install: REFUSING the elevated removal for ', 'updater: install kind decided', 'installing SILENTLY now (setup.exe /S /UPDATE /R /ARGS', 'theme: watching the Windows app light/dark setting every ', 'rival install: this PACKAGED copy found an unpackaged per-user install beside it (PROBLEM 250 follow-up ', 'safe-mode: this process is the surviving single instance ', 'the one-time config snapshot would copy config.json and the backups into a subfolder', 'callback-only counters (PROBLEM 236): nothing but a hook proc can move them', 'the primary keyboard hook saw this Space-down and delivered it', 'own-window fallback: Space-down came from the dashboard page (PROBLEM 257 fallback), hook silent', 'The keyboard hook did NOT see this Space; the page did, and', "PROVEN' and 'own-window fallback:'.", 'MECHANISM (PROBLEM 260): when a WH_KEYBOARD_LL callback overruns LowLevelHooksTimeout', "the old line read a 60s counter and printed '#1' three times). Handles: keyboard ", ' consecutive forced repair(s) delivered no keyboard callback, so the backoff is engaged and the next repair waits ', 'own-window fallback: the re-hook tore down a live fallback hold (PROBLEM 261) ', 'own-window fallback: reaping a fallback Space-hold (', "Left standing this is a ring on screen with the pointer still arming chips behind it, which is PROBLEM 218's failure on the path PROBLEM 218's reaper cannot see", 'own-window-holds-reaped(page stopped talking mid-hold):', 'stale-hold-reaped-because-the-keyboard-is-proven-deaf-spaceadom', 'modifier-active-latched-past-the-bound-with-no-keyboard-callbacks-spaceadom', 'repair-tore-down-a-hold-that-predated-it-spaceadom', 'deferral-episode-bound-expired-proceeding-with-the-repair-spaceadom', 'middle-button ring: the-guide-hud-ring-was-raised-by-a-middle-mouse-button-hold-spaceadom', 'picker-serve-decision-path-and-list-age-marker-spaceadom-237:', 'overlay usable for the Guide HUD', 'startup: logon task registered for this user via the Task Scheduler API', 'middle-button ring v2: cursor-anchored-ring-raised-at-cursor-spaceadom-267', 'voice_typing: sent Win+H (Windows dictation)', 'hook: raw-input keyboard sink registered on the hook thread (hwnd ') {
  Add-Content $Out ("rust marker '{0}': {1}" -f $m, ($bytes -match [regex]::Escape($m)))
}

# The frontend chain: the marker is in the bundle, and the exe postdates it.
#
# 1.0.96 REPLACED TWO ENTRIES, and the reason matters more than the swap.
# 'New ring layout' and 'Shortcut rows' were the two settings controls this
# list watched from 1.0.89. Both were REMOVED on 2026-09-01 when the
# Compact/Wide/Double ring pill replaced them, so both correctly went False in
# the 1.0.96 build — a stale checklist entry measuring a control that no longer
# exists, not a fix that failed to ship. They survive in the source only as
# comments, and vite strips comments, which is why grepping src/ still finds
# them and the bundle does not.
# GENERALISE: a marker list has to be retired alongside the feature it watches,
# or it starts producing Falses that mean nothing and train the reader to
# ignore them. Their replacements are the four IPC command names the 1.0.96
# frontend actually invokes — reorder_profiles, preview_hud_layout,
# run_overlay_fix, duplicate_profile — which cannot go stale without the
# feature going with them.
$assetDir = Join-Path $Root 'dist2\assets'
$bundle = (Get-ChildItem $assetDir -File | ForEach-Object { Get-Content $_.FullName -Raw }) -join "`n"
# 1.0.97 ADDED FOUR, and retired none: all fourteen 1.0.96 entries were
# re-grepped against dist2\assets and every one is still present, so there is
# nothing stale to retire this release. The four additions are the frontend
# features 1.0.97 exists for, each named by something that cannot survive the
# feature being removed:
#   'tour_done'                — the first-run walkthrough's config field.
#   'Show me the walkthrough'  — the settings header link that restarts it.
#   'picker-data-updated'      — the picker's background-refresh event topic.
#   'ed-replace-confirm'       — the paste-over-a-filled-key Replace prompt.
# 1.0.98 ADDED ONE, and retired none: all eighteen 1.0.97 entries were
# re-grepped against dist2\assets and every one is still present.
#   'This only removes the leftover entry from Programs and Features'
#       — PROBLEM 244's new banner sentence, the one that tells the owner what
#         the button will actually do. It was measured ABSENT from the 1.0.97
#         dist2\assets before `npm run build` overwrote them, so a True here is
#         evidence and not a coincidence. It is the THIRD 1.0.98 marker; the
#         other two are Rust and are scanned in the exe above.
# 1.0.101 ADDED ONE, and retired none: all nineteen 1.0.98 entries were
# re-grepped against dist2ssets and every one is still present.
#   'A Microsoft Store copy of Spaceadom is also installed'
#       - the store_copy arm of checkRivalInstall (PROBLEM 250 follow-up), the
#         fourth banner shape. It is the ONE new frontend string this release
#         has, and it is the whole visible half of the feature: the other half
#         is the ABSENCE of a repair button, which no grep can see. NOTE the
#         honest limit on this marker: `npm run build` had already overwritten
#         dist2 before the 1.0.100 baseline could be read, so unlike 1.0.98's
#         banner sentence this one was NOT measured absent from the previous
#         bundle. A True here proves it shipped; it does not prove it is new.
# 1.0.102 ADDED TWO, and retired none: all twenty 1.0.101 entries were
# re-grepped against dist2\assets and every one is still present.
#   '--ind-x'
#       - the CSS custom property the PROBLEM 255 follow-up introduced. The
#         theme pill's sliding highlight used to be positioned by ARITHMETIC
#         (--seg-i x 100% of an assumed equal segment); it is now positioned by
#         MEASUREMENT, and controls.ts writes the measured offset into this
#         variable. The name exists in BOTH halves of the fix - styles.css
#         reads it, controls.ts writes it - so it lands in os-theme CSS and in
#         main JS, and a True here means both halves shipped.
#         It was measured ABSENT from the 1.0.101 dist2\assets bundle before
#         `npm run build` overwrote them? NO - honest limit, same as 1.0.101's
#         banner marker: the build ran first. A True proves it shipped in THIS
#         bundle; the version stamp + exe-newer-than-dist2 links are what prove
#         the installed exe carries THIS bundle.
#   '--ind-w'
#       - the second half of the same pair: the measured WIDTH. Same reason.
# REJECTED MARKER, recorded so nobody re-adds it: 'positionSegIndicator', the
# function that does the measuring, tests FALSE in a bundle that certainly
# contains it - it is a module-scope function and the minifier renames it.
# Same family as CLAUDE.md's short-Rust-literal trap: a marker was tested
# against the freshly-built bundle BEFORE being trusted, and it failed the
# test, so it never shipped as a check. A CSS custom-property NAME cannot be
# renamed (the CSS and the JS have to agree on it at runtime), which is
# exactly why --ind-x / --ind-w are safe markers and the function name is not.
# 1.0.103 ADDED THREE, and retired none: all twenty-two 1.0.102 entries were
# re-grepped against dist2\assets and every one is still present.
#   'has more than one profile. Pick the one this key should open'
#       - tour.ts step2bNamed(), the PROBLEM 242 follow-up's browser-profile
#         beat. A copy string the feature cannot lose and keep working.
#   'profile-undo-btn'
#       - profile-editor.ts, the element id PROBLEM 256's ticking countdown
#         needs in order to rewrite the button's own text node every second.
#         The undo ROW shipped in 1.0.96; the id is what the new clock uses, so
#         it is the half that is genuinely new.
#   'step2b'
#       - the tour Phase literal. A TS string union member survives
#         minification (the comparisons are against string literals), unlike
#         the module-scope function name this list already records as rejected.
# THE HONEST LIMIT, same as 1.0.101's and 1.0.102's frontend markers and worth
# repeating rather than quietly dropping: `npm run build` had already
# overwritten dist2 before any 1.0.102 baseline of the bundle could be read, so
# these three were NOT measured absent from the previous bundle. A True here
# proves they shipped in THIS bundle; the version stamp plus the
# exe-newer-than-dist2 link are what prove the installed exe carries this
# bundle. Rust markers are the ones carrying the absent-then-present half of
# the evidence this release.
# 1.0.104 ADDED THREE, and retired none: all twenty-five 1.0.103 entries were
# re-grepped against dist2ssets and every one is still present.
#   'own_window_space_down'
#       - the IPC command name own-window-keys.ts passes to invoke(). An
#         invoke() argument is a STRING LITERAL that has to survive
#         minification byte-for-byte, because the Rust side matches on it -
#         which is exactly the property the rejected 'positionSegIndicator'
#         marker lacked. It is the whole PROBLEM 259 feature in one token: no
#         fallback without this call.
#   'exc-add-btn'
#       - settings-panel.ts's compacted "Add an app" button (PROBLEM 239
#         follow-up). A CSS class name, so styles.css and the TS must agree on
#         it at runtime and the minifier cannot rename it.
#   'conflict-row-close'
#       - the Conflicts card's new real "Close it" button, same release, same
#         reasoning.
# THIS RELEASE BREAKS THE HONEST LIMIT, for the first frontend marker since
# 1.0.98's banner sentence: 'own_window_space_down' was MEASURED ABSENT from
# the dist2ssets bundle BEFORE `npm run build` overwrote it. That bundle was
# written 2026-09-07 09:54:06 by the PROBLEM 239 follow-up agent; the fallback's
# own-window-keys.ts and main.ts wiring were written at 10:01:41, after it. The
# reading is in _probe.0.104\dist2-baseline-prebuild.txt: 0 hits for each of
# own_window_space_down / own_window_key / own_window_space_up. So a True here
# is absent-then-present evidence, not a coincidence.
# The OTHER TWO keep the honest limit and it is worth saying rather than
# blurring: exc-add-btn and conflict-row-close were already present in that
# 09:54 bundle (2 hits each), because the same agent built it. They were never
# in an INSTALLED exe - 1.0.103 was installed 2026-09-06 01:40, before either
# existed - but this list cannot prove that from the bundle side. A True proves
# they shipped in THIS bundle; the version stamp plus the exe-newer-than-dist2
# link are what prove the installed exe carries this bundle.
# 1.0.106 ADDED TWO, and retired none: all twenty-eight 1.0.104 entries were
# re-grepped against dist2\assets and every one is still present.
#   'conflict-grid'
#       - PROBLEM 239's second pass. The CSS class on the wrapper that turns the
#         Conflicts list from full-width bars into a
#         `repeat(auto-fit, minmax(240px, 1fr))` grid of compact cards. A class
#         name, so styles.css and settings-panel.ts must agree on it at runtime
#         and the minifier cannot rename it - the same property that makes
#         'exc-add-btn' and 'conflict-row-close' safe markers and made
#         'positionSegIndicator' a rejected one.
#   'conflict-row-why'
#       - the clamped description inside each card (`-webkit-line-clamp: 2`,
#         with the full text on a `title` attribute past the clamp). HONEST
#         LIMIT, stated rather than blurred: the CLASS predates this release -
#         the one-liner description came back ungated on 2026-08-20 - so a True
#         here proves the clamped element shipped in THIS bundle, not that the
#         name is new. 'conflict-grid' is the marker carrying the new half.
# THE HONEST LIMIT ON BOTH, and it is wider than 1.0.104's: `npm run build` had
# already been run by the PROBLEM 239 second-pass agent BEFORE this ship task
# started, so no baseline of the 1.0.105-era dist2\assets bundle survives and
# neither marker could be measured ABSENT from it. Worse, the frontend markers
# can never be measured against an INSTALLED exe at all - Tauri v2 compresses
# the embedded dist2, which CLAUDE.md records as measured. So the
# absent-then-present half of this release's evidence is carried entirely by
# the four Rust markers above; what these two prove is that the strings are in
# the bundle the version-stamped, exe-newer-than-dist2 installed binary
# embedded.
foreach ($m in 'st-beam', 'aiming', 'hudspecials', 'hud-layout-changed', 'reorder_profiles', 'preview_hud_layout', 'run_overlay_fix', 'duplicate_profile', 'magnetic', 'bandRx', 'ring EXHAUSTED at step', 'bp-tile-sub', 'st-bp-browsers-v2', 'account_label', 'tour_done', 'Show me the walkthrough', 'picker-data-updated', 'ed-replace-confirm', 'This only removes the leftover entry from Programs and Features', 'A Microsoft Store copy of Spaceadom is also installed', '--ind-x', '--ind-w', 'has more than one profile. Pick the one this key should open', 'profile-undo-btn', 'step2b', 'own_window_space_down', 'exc-add-btn', 'conflict-row-close', 'conflict-grid', 'conflict-row-why', 'guide_arcs', 'Voice Typing') {
  Add-Content $Out ("bundle has {0}: {1}" -f $m, ($bundle -match [regex]::Escape($m)))
}

$newest = Get-ChildItem (Join-Path $Root 'dist2') -Recurse -File |
          Sort-Object LastWriteTime -Descending | Select-Object -First 1
Add-Content $Out ("newest dist2 file: " + $newest.LastWriteTime)
Add-Content $Out ("exe is newer than the bundle it embedded: " +
                  ($item.LastWriteTime -gt $newest.LastWriteTime))

# ---------------------------------------------------------------------------
# REQUIRED MANUAL STEP — CLAUDE.md keyboard-hook law 6 (PROBLEM 257).
#
# THE RING MUST SHOW OVER OUR OWN WINDOW. 1.0.101 and 1.0.102 both shipped
# "proved" while every Space held inside the dashboard was invisible to the
# keyboard hook. Nothing above can see that: markers prove bytes shipped, not
# that a hardware key reached the hook with our window focused.
#
# What passes: in %APPDATA%\Spaceadom\debug.log, AFTER the banner line of the
# build being proved (`Spaceadom build — version <this exe's version>`), a
#     hold start (hold #N): ... over own window
# line (engine/mod.rs — it exists only if the PRIMARY hook saw the Space)
# followed by a
#     guide_hud: shown over own window
# line for the same process. The `hold start` line is REQUIRED because the
# "Check the ring" button prints the shown-over line for a PREVIEW, which
# proves the overlay path and nothing about the hook — that exact preview
# line is what made 1.0.102's log look healthy on 2026-09-06.
#
# Who produces it: the OWNER (or a person at the keyboard) opens the
# dashboard, clicks into it so it has focus, holds Space for a second, lets
# go. An agent CANNOT do this from the container (testing laws: injected
# input never reaches the hook from there), so an agent-run proof that has no
# owner hold must report UNPROVEN, in capitals, in the ship report. This block
# is re-runnable: run install-real.cmd's proof again after the hold.
# ---------------------------------------------------------------------------
# ENCODING TRAP, MEASURED 2026-09-06 — and it is why this block had NEVER RUN.
# This file has no BOM, and install-real.cmd calls it with `powershell` (Windows
# PowerShell 5.1), which reads a BOM-less file as CP1252. A UTF-8 em dash is
# three bytes; the third is 0x94, which CP1252 maps to U+201D — and PowerShell
# accepts a curly quote as a STRING DELIMITER. So an em dash inside a
# DOUBLE-quoted string closed that string early and the whole file failed to
# parse. install-real.cmd printed exactly one line about it,
# "PROOF STEP PRODUCED NOTHING - powershell never ran", which is easy to read as
# a plumbing hiccup rather than "the law-6 proof does not exist". Every
# $ownWindow string below is therefore pure ASCII.
# GENERALISE: the same rule the marker lists already follow for the ASCII exe
# scan applies to the SCRIPT ITSELF — a non-ASCII character in a BOM-less .ps1
# is a parse risk under 5.1, and a proof step that cannot run is not a proof.
$log = Join-Path $env:APPDATA 'Spaceadom\debug.log'
$ver = $item.VersionInfo.FileVersion
$ownWindow = 'FAIL - ' + $log + ' not found'
if (Test-Path $log) {
  $lines = Get-Content $log
  $banner = -1
  for ($i = $lines.Count - 1; $i -ge 0; $i--) {
    if ($lines[$i] -match 'Spaceadom build' -and $lines[$i] -match [regex]::Escape("version $ver ")) { $banner = $i; break }
  }
  if ($banner -lt 0) {
    $ownWindow = "FAIL - no 'Spaceadom build ... version $ver' banner in debug.log: the build being proved has not RUN yet, so nothing can have been held over it"
  } else {
    $holdStart = $null; $shown = $null
    for ($i = $banner; $i -lt $lines.Count; $i++) {
      if (-not $holdStart -and $lines[$i] -match 'hold start \(hold #\d+\)' -and $lines[$i] -match 'over own window') { $holdStart = $lines[$i] }
      if ($holdStart -and $lines[$i] -match 'guide_hud: shown over own window') { $shown = $lines[$i]; break }
    }
    if ($holdStart -and $shown) {
      $ownWindow = 'PASS - ' + $holdStart.Substring(0, [Math]::Min(60, $holdStart.Length)) + ' ... then ' + $shown.Substring(0, [Math]::Min(60, $shown.Length))
    } elseif ($holdStart) {
      $ownWindow = 'FAIL - the hook saw a Space over our own window (' + $holdStart.Substring(0, 23) + ') but no `guide_hud: shown over own window` followed: the fault is between the engine and the HUD, not in the hook'
    } else {
# COUNTER TRAP, MEASURED 2026-09-06 on the 1.0.103 install, and fixed here.
# $deaf below used to match the bare phrase 'KEYBOARD DEAF, PROVEN'. That
# phrase also appears inside the ENGINE's own hold-start line, which ends
# "... grep 'KEYBOARD DEAF, PROVEN'." as advice to the reader. So the counter
# reported 18 deaf events on a run that had ZERO (18 was the number of holds
# the hook had SEEN and handled perfectly), and it inverted: the healthier the
# hook, the more deaf events it claimed. It now matches 'hook: KEYBOARD DEAF,
# PROVEN', the WARN's own prefix, which the advice text cannot contain.
# GENERALISE: a log line that tells the reader what to grep for becomes a hit
# for that grep. A diagnostic string quoted inside another diagnostic is a
# self-match, and a counter that rises with health is worse than no counter.
      $deaf = @($lines[$banner..($lines.Count - 1)] | Where-Object { $_ -match 'hook: KEYBOARD DEAF, PROVEN' }).Count
      $ownWindow = "FAIL - no 'hold start ... over own window' line since the $ver banner. Either nobody has held Space with the dashboard focused yet (do it now, then re-run this proof), or the keyboard hook is not being called while our window has focus (PROBLEM 257; 'KEYBOARD DEAF, PROVEN' lines since the banner: $deaf). Until this reads PASS the ship report MUST say UNPROVEN."
    }
  }
}
Add-Content $Out ("own-window ring proof (law 6, REQUIRED MANUAL STEP): " + $ownWindow)

# ---------------------------------------------------------------------------
# THE SECOND ROW OF LAW 6's TABLE - PROBLEM 259's own-window FALLBACK.
#
# CLAUDE.md keyboard-hook law 6 now carries two proof pairs, and reporting only
# one of them is how a ring seen inside the dashboard gets written up as
# evidence the hook is alive:
#
#   the keyboard HOOK   'hold start (hold #N) ... over own window'
#                       then 'guide_hud: shown over own window'   <- the block
#                                                                    above
#   the FALLBACK        'own-window fallback:'
#                       then 'guide_hud: shown over own window'   <- this one
#
# This line is INFORMATIONAL and can never turn the block above into a PASS:
# it greps a different sentence, and engine/mod.rs deliberately keeps the words
# 'hold start' out of the fallback's line so the two can never be confused.
# Read the two lines together. Fallback SEEN + law 6 FAIL is the expected,
# honest 1.0.104 result on this machine: the ring works inside the dashboard
# and PROBLEM 257 is still open. Fallback SEEN + law 6 PASS for the SAME
# keystroke would mean the Rust dedupe failed (two rings, two launches, two
# spaces per press) - that is a bug, not a curiosity.
# ---------------------------------------------------------------------------
$fallback = 'NOT SEEN - no ' + [char]39 + 'own-window fallback:' + [char]39 + ' line since this build banner'
if (Test-Path $log) {
  $lines = Get-Content $log
  $banner = -1
  for ($i = $lines.Count - 1; $i -ge 0; $i--) {
    if ($lines[$i] -match 'Spaceadom build' -and $lines[$i] -match [regex]::Escape("version $ver ")) { $banner = $i; break }
  }
  if ($banner -lt 0) {
    $fallback = 'NOT SEEN - this build has not RUN yet (no banner in debug.log)'
  } else {
    $fb = $null; $fbShown = $null
    for ($i = $banner; $i -lt $lines.Count; $i++) {
      if (-not $fb -and $lines[$i] -match 'own-window fallback: Space-down came from the dashboard page') { $fb = $lines[$i] }
      if ($fb -and $lines[$i] -match 'guide_hud: shown over own window') { $fbShown = $lines[$i]; break }
    }
    if ($fb -and $fbShown) {
      $fallback = 'SEEN - the page carried the hold and the ring followed: ' + $fb.Substring(0, [Math]::Min(70, $fb.Length))
    } elseif ($fb) {
      $fallback = 'PARTIAL - the fallback took a hold but no ' + [char]39 + 'guide_hud: shown over own window' + [char]39 + ' followed it: the fault is between the engine and the HUD, not in the fallback'
    }
  }
}
Add-Content $Out ("own-window FALLBACK (PROBLEM 259, informational - NEVER satisfies law 6): " + $fallback)

