# ---------------------------------------------------------------------------
# preinstall-probe.ps1 — snapshot the REAL machine BEFORE the 1.0.90 install.
#
# Purpose: prove that the ASCII markers used as evidence for 1.0.88 are
# genuinely NEW. A marker that was already in 1.0.86 proves nothing, so the
# only honest baseline is the exe that is installed RIGHT NOW.
#
# 1.0.88's marker set. The first three are CONTROLS — they shipped long before
# this release, so a False on all of them means the scan technique broke, not
# that a fix is missing. The last two are NEW in 1.0.88 and were confirmed
# PRESENT in the freshly-built exe before this probe ran (2026-08-27), which is
# what makes their absence here meaningful rather than merely unmeasured.
#   '), dead zone '     — a piece of pointer.rs's publish log FORMAT string
#                          (format_args pieces always live in .rodata, so the
#                          short-literal trap below cannot apply to them).
#   'hud_show_specials'  — the new serde field name.
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
# 1.0.90's set. The first FIVE are CONTROLS - every one of them shipped in
# 1.0.88/1.0.89 and is expected TRUE here; a False across all five means the
# scan technique broke, not that a fix is missing.
# The last FOUR are the 1.0.90 candidates (PROBLEM 214/215). All four were
# confirmed PRESENT in the freshly-built 1.0.90 exe at
# the freshly-built 1.0.90 exe in src-tauri targeted release output BEFORE this probe ran (2026-08-28 20:11, ver
# 1.0.90, 18,994,176 bytes) - which is what makes a False here evidence rather
# than an unmeasured guess. Every one is a piece of a log FORMAT string, so
# format_args! guarantees it lives in .rodata and the short-literal
# immediate-store trap below cannot reach it.
#   'hook and engine are LIVE now; ...'  - PROBLEM 215, the autostart split.
#   'ADOPTED and reconfigured it ...'    - PROBLEM 214, the adopt path.
#   'rebuilding the overlay ONCE. ...'   - PROBLEM 214, stability coalescing.
#   'will keep retrying on a backoff; ...' - PROBLEM 214, the self-heal.
# 1.0.93's set. The first FIVE are CONTROLS - every one measured True in the
# installed 1.0.92 (install-check.txt, 2026-08-29 02:30). The last TWO are NEW
# in 1.0.93 (the fullscreen-PiP restore leg) and were confirmed PRESENT in the
# freshly-built 1.0.93 exe (ver 1.0.93, 19,080,704 bytes, 03:14 on 2026-08-29)
# BEFORE this probe ran - which is what makes a False here evidence rather than
# an unmeasured guess. Both are pieces of log FORMAT strings, so format_args!
# puts them in .rodata and the short-literal immediate-store trap cannot reach
# them.
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
