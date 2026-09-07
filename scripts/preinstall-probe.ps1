# ---------------------------------------------------------------------------
# preinstall-probe.ps1 — snapshot the REAL machine BEFORE the next install.
#
# Purpose: prove that the ASCII markers used as evidence for a release are
# genuinely NEW. A marker that was already in the previous build proves nothing,
# so the only honest baseline is the exe that is installed RIGHT NOW.
#
# Run only through preinstall-probe.cmd, launched by explorer.exe (PROBLEM 143).
# ---------------------------------------------------------------------------
param([Parameter(Mandatory = $true)][string]$Out)

Add-Content $Out ("probe ran at: " + (Get-Date -Format 'yyyy-MM-dd HH:mm:ss'))
Add-Content $Out ("LOCALAPPDATA as seen out here: " + $env:LOCALAPPDATA)

$exe = Join-Path $env:LOCALAPPDATA 'Spaceadom\spaceadom.exe'
Add-Content $Out ("exe path: " + $exe)

if (Test-Path $exe) {
  $i = Get-Item $exe
  Add-Content $Out ("PRE version: " + $i.VersionInfo.FileVersion)
  Add-Content $Out ("PRE written: " + $i.LastWriteTime)
  Add-Content $Out ("PRE size: " + $i.Length)
  $bytes = [Text.Encoding]::ASCII.GetString([IO.File]::ReadAllBytes($exe))
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
#
# 1.0.98's set (2026-09-04, night — PROBLEM 244, the msiexec incident).
# The first SIX are CONTROLS: five shipped in 1.0.88..1.0.96, and
# 'rival install: REFUSING the elevated removal for ' shipped in 1.0.97, so a
# TRUE on it also names WHICH build this baseline is. A False across all six
# means the scan technique broke, not that a fix is missing.
# The last TWO are NEW in 1.0.98 and must read False here. Both are pieces of
# log FORMAT strings (format_args! puts those in .rodata, so the short-literal
# immediate-store trap cannot reach them), both pure ASCII, both
# apostrophe-free so a PowerShell single-quoted list is safe:
#   'rival install: REGISTRY-ONLY removal (PROBLEM 244) - deleting the leftover'
#                       — rival_install::repair(), the arm that replaced
#                         msiexec for an orphaned entry.
#   'rival install: REFUSING msiexec /X for this product (PROBLEM 244) - its'
#                       — rival_install::plan_removal()'s refusal reason, the
#                         one that would have saved the app on 2026-09-04.
# Both were confirmed PRESENT in the freshly-built 1.0.98 exe under
# target\release BEFORE this baseline was read — which is what makes a False
# here evidence rather than an unmeasured guess.
#
# The THIRD 1.0.98 marker is FRONTEND ("This only removes the leftover entry
# from Programs and Features") and cannot be scanned in an exe at all: Tauri v2
# compresses the embedded bundle. It was proved absent from the 1.0.97
# dist2\assets before the rebuild overwrote them, and is proved present in the
# 1.0.98 bundle by install-proof.ps1.
#
# 1.0.97's set (2026-09-04, evening). The first FIVE are CONTROLS — every one
# shipped in 1.0.88..1.0.96 and is expected TRUE against the installed 1.0.96;
# a False across all five means the scan technique broke, not that a fix is
# missing. ('not been re-checked since the occlusion fix (PROBLEM 171)…' is the
# 1.0.96 marker, so a True on it also confirms WHICH build the baseline is.)
# The last FOUR are NEW in 1.0.97 (PROBLEMs 236/237/238-review/243). Every one
# is a piece of a log FORMAT string, so format_args! guarantees it lives in
# .rodata and the short-literal immediate-store trap cannot reach it. Each was
# confirmed PRESENT in the freshly-built 1.0.97 exe as well — which is what
# makes a False here evidence rather than an unmeasured guess.
# All four are pure ASCII on purpose: the scan decodes the file as ASCII, so a
# marker containing an em-dash or a curly quote can never match. None of them
# contains an apostrophe either, so a PowerShell single-quoted list is safe.
#   'picker_worker: st-picker-scan started (os thread '
#                       — picker_worker.rs, PROBLEM 237's off-main-thread scan.
#   'hook: WATCHDOG would have alarmed ('
#                       — hook/mod.rs, PROBLEM 236's shadow-verdict line.
#   'rival install: REFUSING the elevated removal for '
#                       — rival_install.rs, the removal_target hard guard.
#   'no own-window check anywhere on this path (PROBLEM 243): not in the hook'
#                       — guide_hud/mod_impl.rs, the shown-over line.
# NOTE, and it is why 'guide_hud: shown over own window' is NOT the marker:
# that sentence is ASSEMBLED at runtime from a format piece plus the return of
# shown_over_phrase(), so it never sits contiguously on disk. Only the literal
# FORMAT piece can be scanned for.
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
foreach ($m in 'rival install', 'start_menu_scan:', 'hud-band-count-changed', 'restored to TRUE FULLSCREEN', 'picker_worker: st-picker-scan started (os thread ', 'rival install: REFUSING the elevated removal for ', 'updater: install kind decided', 'installing SILENTLY now (setup.exe /S /UPDATE /R /ARGS', 'theme: watching the Windows app light/dark setting every ', 'rival install: this PACKAGED copy found an unpackaged per-user install beside it (PROBLEM 250 follow-up ', 'safe-mode: this process is the surviving single instance ', 'the one-time config snapshot would copy config.json and the backups into a subfolder', 'callback-only counters (PROBLEM 236): nothing but a hook proc can move them', 'the primary keyboard hook saw this Space-down and delivered it', 'own-window fallback: Space-down came from the dashboard page (PROBLEM 257 fallback), hook silent', 'The keyboard hook did NOT see this Space; the page did, and', "PROVEN' and 'own-window fallback:'.", 'MECHANISM (PROBLEM 260): when a WH_KEYBOARD_LL callback overruns LowLevelHooksTimeout', "the old line read a 60s counter and printed '#1' three times). Handles: keyboard ", ' consecutive forced repair(s) delivered no keyboard callback, so the backoff is engaged and the next repair waits ', 'own-window fallback: the re-hook tore down a live fallback hold (PROBLEM 261) ', 'own-window fallback: reaping a fallback Space-hold (', "Left standing this is a ring on screen with the pointer still arming chips behind it, which is PROBLEM 218's failure on the path PROBLEM 218's reaper cannot see", 'own-window-holds-reaped(page stopped talking mid-hold):', 'stale-hold-reaped-because-the-keyboard-is-proven-deaf-spaceadom', 'modifier-active-latched-past-the-bound-with-no-keyboard-callbacks-spaceadom', 'repair-tore-down-a-hold-that-predated-it-spaceadom', 'deferral-episode-bound-expired-proceeding-with-the-repair-spaceadom') {
    Add-Content $Out ("PRE marker '{0}': {1}" -f $m, ($bytes -match [regex]::Escape($m)))
  }
} else {
  Add-Content $Out "PRE: no exe installed"
}

$run = (Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' `
          -Name Spaceadom -ErrorAction SilentlyContinue).Spaceadom
Add-Content $Out ("PRE Run key: " + $run)

$procs = Get-Process spaceadom -ErrorAction SilentlyContinue
if ($procs) {
  foreach ($p in $procs) {
    Add-Content $Out ("PRE running: pid={0} path={1} started={2}" -f $p.Id, $p.Path, $p.StartTime)
  }
} else {
  Add-Content $Out "PRE running: none"
}

$log = Join-Path $env:APPDATA 'Spaceadom\debug.log'
if (Test-Path $log) {
  Add-Content $Out ("PRE debug.log size: " + (Get-Item $log).Length + "  lastWrite: " + (Get-Item $log).LastWriteTime)
} else {
  Add-Content $Out "PRE debug.log: absent"
}

# config.json is SHADOWED in the agent shell even though debug.log beside it is
# not (CLAUDE.md, observed twice). Copy the REAL one out to D: — a drive the
# container does not redirect — so its byte size can be cross-checked against
# the last `config: saved N bytes` line in debug.log. Read-only: this copies
# OUT, it never writes back. A live config is not ours to touch.
$cfg = Join-Path $env:APPDATA 'Spaceadom\config.json'
if (Test-Path $cfg) {
  $c = Get-Item $cfg
  Add-Content $Out ("PRE config.json size: " + $c.Length + "  lastWrite: " + $c.LastWriteTime)
  # 1.0.98: a SIZE match is not a byte-identical match. Hash it here and hash
  # it again after the install; equal SHA-256 is the only claim worth making
  # about someone else's live data.
  Add-Content $Out ("PRE config.json SHA256: " + (Get-FileHash $cfg -Algorithm SHA256).Hash)
  $preCopy = 'D:\Claude-Projects\SpaceToggle-V14\_config-live-copy-1.0.107-pre.json'
  Copy-Item $cfg $preCopy -Force
  Add-Content $Out ("PRE config copied out to: " + $preCopy)
  # Profile count, read from the copy, so the post-install count can be
  # compared against something rather than asserted from memory.
  try {
    $j = Get-Content $preCopy -Raw | ConvertFrom-Json
    $names = @($j.profiles | ForEach-Object { $_.name })
    Add-Content $Out ("PRE profiles: " + $names.Count + "  [" + ($names -join ', ') + "]")
  } catch {
    Add-Content $Out ("PRE profiles: could not parse config.json - " + $_.Exception.Message)
  }
} else {
  Add-Content $Out "PRE config.json: absent"
}
