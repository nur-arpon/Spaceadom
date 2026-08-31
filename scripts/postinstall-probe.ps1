# ---------------------------------------------------------------------------
# postinstall-probe.ps1 — the "is it actually RUNNING" half of the 1.0.90 proof.
#
# install-real.cmd proves the FILE on disk. It cannot prove the owner is now
# looking at that build, because it starts the exe on its last line and exits.
# This runs afterwards, from outside the MSIX container (PROBLEM 143), and
# reports the live PID, the path it booted from, and the startup block the new
# process wrote to debug.log.
#
# Reads only. It never touches config.json — that is the owner's live config.
# ---------------------------------------------------------------------------
param([Parameter(Mandatory = $true)][string]$Out)

Add-Content $Out ("probe ran at: " + (Get-Date -Format 'yyyy-MM-dd HH:mm:ss'))

$exe = Join-Path $env:LOCALAPPDATA 'Spaceadom\spaceadom.exe'
$i = Get-Item $exe
Add-Content $Out ("POST version: " + $i.VersionInfo.FileVersion)
Add-Content $Out ("POST written: " + $i.LastWriteTime)
Add-Content $Out ("POST size: " + $i.Length)

# Give the freshly-started app a moment to open its log, but do not assume:
# report what is actually there either way.
$procs = $null
for ($n = 0; $n -lt 20; $n++) {
  $procs = Get-Process spaceadom -ErrorAction SilentlyContinue
  if ($procs) { break }
  Start-Sleep -Milliseconds 500
}
if ($procs) {
  foreach ($p in $procs) {
    Add-Content $Out ("POST running: pid={0} path={1} started={2}" -f $p.Id, $p.Path, $p.StartTime)
  }
} else {
  Add-Content $Out "POST running: NONE - the app is not up"
}

$log = Join-Path $env:APPDATA 'Spaceadom\debug.log'
if (Test-Path $log) {
  $li = Get-Item $log
  Add-Content $Out ("POST debug.log size: " + $li.Length + "  lastWrite: " + $li.LastWriteTime)
  # STARTUP TIME, from the boot instrumentation. The span is logger-init ->
  # "bootstrap complete, calling dashboard_ready": 1.0.88's last boot measured
  # 10:30:09.439 -> 10:30:10.700 = 1261ms. Report the number, never bury it.
  $lines = Get-Content $log
  $bootIdx = ($lines | Select-String -Pattern 'logger initialised' | Select-Object -Last 1).LineNumber
  if ($bootIdx) {
    $bootLine = $lines[$bootIdx - 1]
    $readyLine = ($lines | Select-Object -Skip ($bootIdx - 1) |
                  Select-String -Pattern 'bootstrap complete, calling dashboard_ready' |
                  Select-Object -First 1).Line
    Add-Content $Out ("STARTUP boot line : " + $bootLine)
    if ($readyLine) {
      Add-Content $Out ("STARTUP ready line: " + $readyLine)
      $t0 = [datetime]::ParseExact($bootLine.Substring(0,23),  'yyyy-MM-dd HH:mm:ss.fff', $null)
      $t1 = [datetime]::ParseExact($readyLine.Substring(0,23), 'yyyy-MM-dd HH:mm:ss.fff', $null)
      Add-Content $Out ("STARTUP ms (1.0.89 baseline was 1474; 1.0.90 changes WINDOW CREATION so a shift is expected): " + [int]($t1 - $t0).TotalMilliseconds)
    } else {
      Add-Content $Out "STARTUP ms: dashboard_ready has not been logged yet for this boot"
    }
  } else {
    Add-Content $Out "STARTUP: no logger-init line found"
  }

  # Panics / errors since this boot. A quiet log is only good news if the scan
  # can actually produce a hit, so the count is printed either way.
  if ($bootIdx) {
    $since = $lines | Select-Object -Skip ($bootIdx - 1)
    $bad = $since | Select-String -Pattern '\[ERROR\]|panic|PANIC|thread .* panicked'
    Add-Content $Out ("POST errors/panics since this boot: " + @($bad).Count)
    @($bad) | Select-Object -First 15 | ForEach-Object { Add-Content $Out ("  ! " + $_.Line) }
    $warns = $since | Select-String -Pattern '\[WARN\]'
    Add-Content $Out ("POST warnings since this boot: " + @($warns).Count)
  }

  Add-Content $Out "---- last 45 lines of debug.log ----"
  Get-Content $log -Tail 45 | ForEach-Object { Add-Content $Out $_ }
} else {
  Add-Content $Out "POST debug.log: absent"
}

# ---------------------------------------------------------------------------
# 1.0.90 ADDITION — OVERLAY ALIVE CHECK (PROBLEM 214).
# This release exists to stop the overlay being switched off by a rebuild race.
# If the overlay is disabled on a CLEAN boot, that is a regression in the very
# fix being shipped. Scan the CURRENT boot only. A scan that cannot produce a
# hit is not a scan, so the counts are printed either way, and the positive
# control is the 'overlay: configured' line itself.
# ---------------------------------------------------------------------------
if ($bootIdx) {
  $since = $lines | Select-Object -Skip ($bootIdx - 1)
  $configured = @($since | Select-String -Pattern 'overlay: configured')
  $failed     = @($since | Select-String -Pattern 'REBUILD FAILED')
  $disabled   = @($since | Select-String -Pattern 'OVERLAY_DISABLED|overlay is DISABLED')
  Add-Content $Out ("OVERLAY 'overlay: configured' lines this boot: " + $configured.Count)
  $configured | ForEach-Object { Add-Content $Out ("  + " + $_.Line) }
  Add-Content $Out ("OVERLAY 'REBUILD FAILED' lines this boot: " + $failed.Count)
  $failed | ForEach-Object { Add-Content $Out ("  ! " + $_.Line) }
  Add-Content $Out ("OVERLAY 'OVERLAY_DISABLED' lines this boot: " + $disabled.Count)
  $disabled | ForEach-Object { Add-Content $Out ("  ! " + $_.Line) }
  Add-Content $Out ("OVERLAY VERDICT alive: " +
                    (($configured.Count -ge 1) -and ($failed.Count -eq 0) -and ($disabled.Count -eq 0)))
  # PROBLEM 215's two window-creation lines, for the record.
  @($since | Select-String -Pattern 'windows created and configured|hook and engine are LIVE|create_app_windows') |
    ForEach-Object { Add-Content $Out ("  W " + $_.Line) }
}

# ---------------------------------------------------------------------------
# 1.0.90 ADDITION — HIS DATA. config.json is COPIED OUT, never modified.
# The agent shell serves a frozen 47,754-byte shadow of it (observed twice),
# so the only trustworthy read is one taken out here, cross-checked against
# what debug.log says was last SAVED.
# ---------------------------------------------------------------------------
$cfg = Join-Path $env:APPDATA 'Spaceadom\config.json'
if (Test-Path $cfg) {
  $ci = Get-Item $cfg
  Add-Content $Out ("CONFIG real size: " + $ci.Length + "  lastWrite: " + $ci.LastWriteTime)
  Copy-Item $cfg 'D:\Claude-Projects\SpaceToggle-V14\config-copy.json' -Force
  Add-Content $Out ("CONFIG copied to D:\Claude-Projects\SpaceToggle-V14\config-copy.json")
  $saved = @($lines | Select-String -Pattern 'config: saved')
  Add-Content $Out ("CONFIG 'config: saved' lines in log: " + $saved.Count)
  $saved | Select-Object -Last 3 | ForEach-Object { Add-Content $Out ("  S " + $_.Line) }
} else {
  Add-Content $Out "CONFIG: absent"
}
