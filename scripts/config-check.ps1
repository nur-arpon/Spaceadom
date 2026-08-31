# ---------------------------------------------------------------------------
# config-check.ps1 — read the owner's LIVE config.json without touching it.
#
# Why this exists: %APPDATA%\Spaceadom\config.json is SHADOWED for the agent
# shell even though debug.log beside it is not (observed 2026-08-26/27 — the
# shell read a months-stale 47,754-byte copy against a real file of 62,463).
# So the usual tell, "this whole folder looks stale", is absent, and no
# diagnosis may be made from a config.json read inside the sandbox.
#
# This script runs OUTSIDE the MSIX container (launched by explorer.exe via
# config-check.cmd, PROBLEM 143). It COPIES the file to a D: path — a drive the
# container does not redirect — and records the real size so the copy can be
# cross-checked against what debug.log says was last saved. If those two
# disagree, the copy is a shadow and must not be reported on.
#
# READ ONLY. It never writes to config.json. That file is live and the owner
# is using this machine.
# ---------------------------------------------------------------------------
param(
  [Parameter(Mandatory = $true)][string]$Out,
  [Parameter(Mandatory = $true)][string]$Copy
)

$dir = Join-Path $env:APPDATA 'Spaceadom'
$cfg = Join-Path $dir 'config.json'
$log = Join-Path $dir 'debug.log'

Add-Content $Out ("check ran at: " + (Get-Date -Format 'yyyy-MM-dd HH:mm:ss'))
Add-Content $Out ("APPDATA out here: " + $env:APPDATA)

if (Test-Path $cfg) {
  $c = Get-Item $cfg
  Add-Content $Out ("REAL config.json size: " + $c.Length)
  Add-Content $Out ("REAL config.json lastWrite: " + $c.LastWriteTime)
  Copy-Item $cfg $Copy -Force
  Add-Content $Out ("copied to: " + $Copy + "  (" + (Get-Item $Copy).Length + " bytes)")
} else {
  Add-Content $Out "REAL config.json: ABSENT"
}

# The cross-check: what does the app itself say it last saved?
if (Test-Path $log) {
  $l = Get-Item $log
  Add-Content $Out ("REAL debug.log size: " + $l.Length + "  lastWrite: " + $l.LastWriteTime)
  Add-Content $Out "---- last 6 'config: saved' lines ----"
  $saved = Select-String -Path $log -Pattern 'config: saved' -ErrorAction SilentlyContinue |
           Select-Object -Last 6
  if ($saved) { $saved | ForEach-Object { Add-Content $Out $_.Line } }
  else { Add-Content $Out "(no 'config: saved' line in the log)" }

  Add-Content $Out "---- alarm scan over the whole log ----"
  foreach ($pat in 'panic', 'cannot move state from Destroyed', 'config parse',
                   'failed to parse', 'migrat', 'overlay_compositing', 'hook.*deaf',
                   'BACKUP', 'recovered') {
    $hits = Select-String -Path $log -Pattern $pat -ErrorAction SilentlyContinue
    Add-Content $Out ("  /{0}/ : {1} hit(s)" -f $pat, ($hits | Measure-Object).Count)
    if ($hits) { $hits | Select-Object -Last 3 | ForEach-Object { Add-Content $Out ("      " + $_.Line) } }
  }

  Add-Content $Out "---- last 60 lines of debug.log ----"
  Get-Content $log -Tail 60 | ForEach-Object { Add-Content $Out $_ }
} else {
  Add-Content $Out "REAL debug.log: ABSENT"
}
