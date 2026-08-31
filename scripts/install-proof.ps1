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
foreach ($m in 'rival install', 'start_menu_scan:', 'hud-band-count-changed', 'hook and engine are LIVE now; only the window/webview creation waits', 'will keep retrying on a backoff; no restart is required', 'restored to TRUE FULLSCREEN', 'A window is only ever held by ONE of the two keys (PROBLEM 220), so the', 'signed in and are labelled by the local part of their account', 'keep the browser''s own display name (no address is ever logged)') {
  Add-Content $Out ("rust marker '{0}': {1}" -f $m, ($bytes -match [regex]::Escape($m)))
}

# The frontend chain: the marker is in the bundle, and the exe postdates it.
$assetDir = Join-Path $Root 'dist2\assets'
$bundle = (Get-ChildItem $assetDir -File | ForEach-Object { Get-Content $_.FullName -Raw }) -join "`n"
foreach ($m in 'st-beam', 'aiming', 'hudspecials', 'hud-layout-changed', 'New ring layout', 'Shortcut rows', 'magnetic', 'bandRx', 'ring EXHAUSTED at step', 'bp-tile-sub', 'st-bp-browsers-v2', 'account_label') {
  Add-Content $Out ("bundle has {0}: {1}" -f $m, ($bundle -match [regex]::Escape($m)))
}

$newest = Get-ChildItem (Join-Path $Root 'dist2') -Recurse -File |
          Sort-Object LastWriteTime -Descending | Select-Object -First 1
Add-Content $Out ("newest dist2 file: " + $newest.LastWriteTime)
Add-Content $Out ("exe is newer than the bundle it embedded: " +
                  ($item.LastWriteTime -gt $newest.LastWriteTime))
