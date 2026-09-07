# ---------------------------------------------------------------------------
# postinstall-probe.ps1 — the "is it actually RUNNING" half of the ship proof.
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
  # 1.0.98: SIZE is not IDENTITY. Hash the live file and compare it against the
  # hash preinstall-probe took before the installer ran. Equal SHA-256 is the
  # only honest way to say "his config was not touched" — and after PROBLEM 244
  # deleted the app out from under him, an unproved claim about his data is
  # worth less than none.
  Add-Content $Out ("CONFIG SHA256: " + (Get-FileHash $cfg -Algorithm SHA256).Hash)
  $preTxt = 'D:\Claude-Projects\SpaceToggle-V14\preinstall-probe.txt'
  if (Test-Path $preTxt) {
    $preHash = (Select-String -Path $preTxt -Pattern 'PRE config.json SHA256: (\w+)' |
                Select-Object -Last 1).Matches.Groups[1].Value
    $postHash = (Get-FileHash $cfg -Algorithm SHA256).Hash
    Add-Content $Out ("CONFIG PRE SHA256 (from the baseline probe): " + $preHash)
    Add-Content $Out ("CONFIG VERDICT byte-identical across the install: " +
                      ($preHash -and ($preHash -eq $postHash)))
  } else {
    Add-Content $Out "CONFIG: no baseline probe file - cannot compare hashes"
  }
  try {
    $cj = Get-Content $cfg -Raw | ConvertFrom-Json
    $pnames = @($cj.profiles | ForEach-Object { $_.name })
    Add-Content $Out ("CONFIG profiles: " + $pnames.Count + "  [" + ($pnames -join ', ') + "]")
  } catch {
    Add-Content $Out ("CONFIG profiles: could not parse - " + $_.Exception.Message)
  }
  $saved = @($lines | Select-String -Pattern 'config: saved')
  Add-Content $Out ("CONFIG 'config: saved' lines in log: " + $saved.Count)
  $saved | Select-Object -Last 3 | ForEach-Object { Add-Content $Out ("  S " + $_.Line) }
} else {
  Add-Content $Out "CONFIG: absent"
}

# ---------------------------------------------------------------------------
# 1.0.98 ADDITION — PROBLEM 244. TWO CHECKS, AND THE SECOND IS THE POINT.
#
# 1. The rival scan's own verdict this boot. On this machine the orphaned HKLM
#    entry is GONE (msiexec /X removed it on 2026-09-04 19:46, which is the
#    incident), so the expected line is
#      'rival install: no second copy found - this machine has one Spaceadom'
#    Any other line means the banner is being offered again and must be read
#    before anyone clicks it. The count is printed either way; the positive
#    control is that SOME 'rival install' line exists at all, because scan()
#    always logs exactly one.
#
# 2. THE MsiInstaller EVENT LOG, over this install's own window. The whole
#    release exists because Windows Installer removed the live app. So the
#    proof that 1.0.98 shipped safely is not only that the exe is there — it
#    is that INSTALLING it produced no Windows Installer activity at all.
#    NOTE the control: building the .msi logs 11707+1033 five to ten seconds
#    after the .msi's mtime (WiX light.exe validates by running the package
#    through the installer engine). Those are BUILD events, they carry no
#    1040/1042 transaction pair, and they happen before this probe's window.
#    A real install or removal always carries 1040 + 1042. Both counts are
#    printed so a 0 can be told from a scan that never ran.
# ---------------------------------------------------------------------------
if ($bootIdx) {
  $rival = @(($lines | Select-Object -Skip ($bootIdx - 1)) | Select-String -Pattern 'rival install:')
  Add-Content $Out ("RIVAL 'rival install:' lines this boot: " + $rival.Count)
  $rival | ForEach-Object { Add-Content $Out ("  + " + $_.Line) }
  $clean = @($rival | Where-Object { $_.Line -match 'no second copy found' })
  Add-Content $Out ("RIVAL VERDICT scan says one Spaceadom: " + ($clean.Count -ge 1))
}

$PROVIDERS = @('MsiInstaller', 'Microsoft-Windows-RestartManager')
$stampFile = 'D:\Claude-Projects\SpaceToggle-V14\_install-window-start.txt'
$winStart = $null
if (Test-Path $stampFile) {
  try { $winStart = [datetime]::ParseExact((Get-Content $stampFile -Raw).Trim(), 'yyyy-MM-dd HH:mm:ss', $null) } catch { }
}
if ($winStart) {
  Add-Content $Out ("MSI install window starts at (stamped by install-real.cmd): " + $winStart)
} else {
  $winStart = (Get-Date).AddMinutes(-10)
  Add-Content $Out ("MSI install window: NO STAMP FILE - falling back to the last 10 minutes (" + $winStart + ")")
}

# The two-hour listing is context, not the verdict. It will normally contain
# this release's own BUILD events: WiX light.exe validates the .msi it just
# wrote by running it through the Windows Installer engine, which logs
# 11707 + 1033 "installed the product" five to ten seconds after the .msi
# file's mtime. Those carry NO 1040/1042 transaction pair; a real install or
# removal always does. Read the pair, not the word "installed".
$context = @(Get-WinEvent -FilterHashtable @{LogName='Application'; ProviderName=$PROVIDERS; StartTime=(Get-Date).AddHours(-2)} -EA SilentlyContinue)
Add-Content $Out ("MSI CONTEXT MsiInstaller/RestartManager events in the last 2 hours: " + $context.Count +
                  "  (must be > 0 for the window count below to mean anything - it includes this build)")
$context | Sort-Object TimeCreated | ForEach-Object {
  Add-Content $Out ("  . {0} {1} id={2} {3}" -f $_.TimeCreated, $_.ProviderName, $_.Id,
                    (($_.Message -replace '\s+',' ')))
}

$msi = @($context | Where-Object { $_.TimeCreated -ge $winStart })
Add-Content $Out ("MSI events inside the install window: " + $msi.Count)
$msi | Sort-Object TimeCreated | ForEach-Object {
  Add-Content $Out ("  ! {0} {1} id={2} {3}" -f $_.TimeCreated, $_.ProviderName, $_.Id,
                    (($_.Message -replace '\s+',' ')))
}
$spaceadomMsi = @($msi | Where-Object { $_.Message -match 'Spaceadom' })
Add-Content $Out ("MSI Spaceadom-named events inside the install window (MUST be 0): " + $spaceadomMsi.Count)
Add-Content $Out ("MSI VERDICT no Windows Installer touched Spaceadom during this install: " +
                  ($spaceadomMsi.Count -eq 0))

# ---------------------------------------------------------------------------
# 1.0.96 ADDITION — THE HOOK CHAIN, PROBLEM 230.
#
# The reference hook is now installed FIRST so it lands at the TAIL of the
# chain, behind the primary. Before this, it was installed last, sat in FRONT
# of the primary, and — because CallNextHookEx is synchronous — its measured
# wall-clock duration included the whole primary callback, which made the one
# hook documented as un-evictable the first one Windows dropped. That is what
# killed shortcuts mid-hold.
#
# There is deliberately NO "installed first" success line to grep: the order is
# structural, not announced. What IS greppable is the pair below, and the pair
# is the check:
#   * 'hook: WH_KEYBOARD_LL + WH_MOUSE_LL installed'  must be PRESENT
#       — the primary pair went in. This is also the positive control: if this
#         is absent the scan is broken, not the hook.
#   * 'REFERENCE keyboard hook failed to install'     must be ABSENT
#       — PROBLEM 230 added this WARN precisely because a failed reference
#         install used to be indistinguishable in the log from a hook that
#         installed fine and went quiet. Its silence is now informative, which
#         it was not before this release.
# Counts are printed either way; a check that cannot produce a negative is not
# a check.
# ---------------------------------------------------------------------------
if ($bootIdx) {
  $since = $lines | Select-Object -Skip ($bootIdx - 1)
  $hookIn  = @($since | Select-String -Pattern 'WH_KEYBOARD_LL \+ WH_MOUSE_LL installed')
  $refFail = @($since | Select-String -Pattern 'REFERENCE keyboard hook failed to install')
  Add-Content $Out ("HOOK install lines this boot: " + $hookIn.Count)
  $hookIn | ForEach-Object { Add-Content $Out ("  + " + $_.Line) }
  Add-Content $Out ("HOOK reference-install FAILURES this boot: " + $refFail.Count)
  $refFail | ForEach-Object { Add-Content $Out ("  ! " + $_.Line) }
  Add-Content $Out ("HOOK VERDICT reference hook installed (first, at the tail): " +
                    (($hookIn.Count -ge 1) -and ($refFail.Count -eq 0)))
  # The watchdog's own verdicts, if it has spoken yet this boot.
  @($since | Select-String -Pattern 'hook: DEAF|reference hook (has NEVER|last genuinely)') |
    Select-Object -First 5 | ForEach-Object { Add-Content $Out ("  W " + $_.Line) }
}

# ---------------------------------------------------------------------------
# 1.0.96 ADDITION — THE ONE-SHOT SOFTWARE-OVERLAY RE-TEST.
#
# `commands::retest_software_overlay_once` re-runs the ring measurement ONCE on
# a machine that a pre-PROBLEM-171 build wrongly flipped into software
# rendering. It must fire exactly once, or say why it did not.
#
# READ THE EARLY RETURN BEFORE READING THE COUNT. The function's first act is
#   if mode != "software" { return; }
# and that return is SILENT ON PURPOSE — it is the overwhelmingly common case
# and logging it every launch would be noise. So on a machine whose
# `overlay_compositing` is "auto", ZERO overlay-fix lines is the CORRECT
# result, not a missing feature, and the log cannot be made to say so. The
# config value is printed here alongside the count so the two are read
# together; without it a 0 is ambiguous, and an ambiguous number is not proof.
# ---------------------------------------------------------------------------
if ($bootIdx -and (Test-Path $cfg)) {
  $mode = (Get-Content $cfg -Raw | ConvertFrom-Json).overlay_compositing
  Add-Content $Out ("RETEST overlay_compositing in the live config: " + $mode)
  $since = $lines | Select-Object -Skip ($bootIdx - 1)
  $retest = @($since | Select-String -Pattern 'overlay-fix: this machine is in SOFTWARE rendering|overlay-fix \(first-launch re-test\)|overlay-fix: this machine is in software rendering and has not been re-checked')
  Add-Content $Out ("RETEST one-shot lines this boot: " + $retest.Count)
  $retest | ForEach-Object { Add-Content $Out ("  + " + $_.Line) }
  $marker = Join-Path $env:APPDATA 'Spaceadom\overlay-recheck-1.done'
  Add-Content $Out ("RETEST marker file present: " + (Test-Path $marker))
  if ($mode -eq 'software') {
    Add-Content $Out ("RETEST VERDICT: expected exactly 1 announce line; got " + $retest.Count)
  } else {
    Add-Content $Out ("RETEST VERDICT: SKIPPED BY DESIGN - overlay_compositing is '" + $mode +
                      "', not 'software', so the one-shot returns before logging anything. " +
                      "0 lines is correct here and the skip itself is not logged.")
  }
}

# ---------------------------------------------------------------------------
# 1.0.97 ADDITION — THE FIRST-RUN CHECKS THIS RELEASE IS ABOUT.
#
# Three of 1.0.97's four headline fixes announce themselves in the log on the
# very first boot, so they can be proved here rather than left to the owner:
#
#   A. PROBLEM 237 — the Start-Menu scan moved OFF the main thread. The proof
#      is not that the line exists, it is that the thread id in it is NOT the
#      process's main thread. So the id is parsed out and compared against the
#      earliest-started thread of the live process (which is the main thread).
#      A line without that comparison would be a claim, not a measurement.
#      A COLD scan on this boot is EXPECTED once: the disk cache is keyed by a
#      Start-Menu fingerprint and 1.0.96 never wrote one.
#
#   B. PROBLEM 236 — the watchdog now needs real evidence before it re-hooks,
#      and nothing inside a 10 s post-install grace may alarm at all. Both
#      windows are counted: 0 required in the first 10 s, and the first 180 s
#      count is REPORTED rather than asserted, because a real eviction during
#      a normal minute of typing is a legitimate alarm and this probe cannot
#      tell the owner's hands from an idle desk.
#
#   C. PROBLEM 243 — 'guide_hud: shown over …' is written at HUD show time, so
#      it CANNOT appear until the owner holds Space. Zero here is the correct
#      and expected result on a fresh boot; it is printed anyway so the number
#      is never mistaken for a silent failure.
#
# Every count is printed whether or not it is zero, and each block names its
# own positive control. A check that cannot produce a negative is not a check.
# ---------------------------------------------------------------------------
if ($bootIdx) {
  $since = $lines | Select-Object -Skip ($bootIdx - 1)

  # ---- A. the picker worker ------------------------------------------------
  $started = @($since | Select-String -Pattern 'picker_worker: st-picker-scan started')
  Add-Content $Out ("PICKER 'st-picker-scan started' lines this boot: " + $started.Count)
  $started | ForEach-Object { Add-Content $Out ("  + " + $_.Line) }

  $mainTid = $null
  $wtid = $null
  if ($started.Count -ge 1) {
    if ($started[0].Line -match 'os thread (\d+), STA joined: (\w+)') {
      $wtid = [int]$Matches[1]
      Add-Content $Out ("PICKER worker os thread id: " + $wtid)
      Add-Content $Out ("PICKER STA joined: " + $Matches[2])
    } else {
      Add-Content $Out "PICKER: the started line did not parse - format changed?"
    }
  }
  $pp = Get-Process spaceadom -ErrorAction SilentlyContinue | Select-Object -First 1
  if ($pp) {
    $mt = $pp.Threads | Sort-Object StartTime | Select-Object -First 1
    if ($mt) {
      $mainTid = [int]$mt.Id
      Add-Content $Out ("PICKER main thread id (earliest-started thread of pid " + $pp.Id + "): " + $mainTid)
    }
  }
  if ($wtid -and $mainTid) {
    Add-Content $Out ("PICKER VERDICT scan is OFF the main thread: " + ($wtid -ne $mainTid))
  } else {
    Add-Content $Out "PICKER VERDICT: could not compare - one of the two ids is missing"
  }

  $warm = @($since | Select-String -Pattern 'picker_warm:')
  Add-Content $Out ("PICKER 'picker_warm:' lines this boot: " + $warm.Count)
  $warm | ForEach-Object { Add-Content $Out ("  + " + $_.Line) }

  $scan = @($since | Select-String -Pattern 'start_menu_scan:')
  Add-Content $Out ("PICKER 'start_menu_scan:' lines this boot: " + $scan.Count)
  $scan | ForEach-Object { Add-Content $Out ("  + " + $_.Line) }
  $cold = @($scan | Where-Object { $_.Line -match 'scanning on worker thread' })
  $warmServed = @($scan | Where-Object { $_.Line -match 'served .* app\(s\) from' })
  Add-Content $Out ("PICKER cold scans this boot (1 is expected on the first boot of a new version - the cache fingerprint includes it): " + $cold.Count)
  Add-Content $Out ("PICKER cache-served answers this boot: " + $warmServed.Count)

  # ---- B. the watchdog -----------------------------------------------------
  # 'WATCHDOG — ' is the ALARM (it re-hooks). 'would have alarmed' is the
  # shadow verdict that only logs, and 'WATCHDOG would re-hook' is the cooldown
  # HOLD-OFF, which is not an alarm at all. All three are counted separately.
  #
  # MEASURED 2026-09-04, AND THE REASON THESE PATTERNS LOOK ODD: the first
  # version of this block matched on 'WATCHDOG . user active', with '.' standing
  # in for the em-dash. It returned 0 against a log holding 2,312 WATCHDOG
  # lines. Cause: the .cmd calls `powershell` (5.1), whose Get-Content decodes
  # this UTF-8 log as ANSI, so every em-dash arrives as the THREE characters
  # 'â€"' and a one-character wildcard cannot span it. A false 0 that looks
  # exactly like a clean boot.
  # THE FIX IS TWO-PART, and both halves are required:
  #   1. Match only on stretches of the alarm sentences that contain NO
  #      non-ASCII character, so the decoding cannot matter either way.
  #   2. Count the SAME pattern across the WHOLE file, every boot included, and
  #      print it as a control. The previous boot is still in this log and it
  #      is known to contain alarms, so a non-zero control beside a zero
  #      this-boot count is what makes the zero evidence. Without it the two
  #      readings are indistinguishable.
  # GENERALISE: same family as the ASCII-marker and MSIX-container traps — a
  # check that cannot produce a truthful negative is not a check, and an
  # encoding is part of the check.
  $ALARM_RE = 'the KEYBOARD hook alone was evicted|but NEITHER hook saw anything'
  $t0w = $null
  try { $t0w = [datetime]::ParseExact($bootLine.Substring(0,23), 'yyyy-MM-dd HH:mm:ss.fff', $null) } catch { }
  $alarms = @($since | Select-String -Pattern $ALARM_RE)
  $allAlarms = @($lines | Select-String -Pattern $ALARM_RE)
  $holdOff = @($since | Select-String -Pattern 'WATCHDOG would re-hook')
  $shadow = @($since | Select-String -Pattern 'would have alarmed')
  $held   = @($since | Select-String -Pattern 'a Space hold is LIVE')
  Add-Content $Out ("WATCHDOG CONTROL alarm lines in the WHOLE log, all boots: " + $allAlarms.Count +
                    "  (must be > 0 for a 0 below to mean anything)")
  Add-Content $Out ("WATCHDOG cooldown hold-off lines this boot (not alarms): " + $holdOff.Count)
  Add-Content $Out ("WATCHDOG total alarm lines this boot: " + $alarms.Count)
  Add-Content $Out ("WATCHDOG shadow 'would have alarmed' lines this boot: " + $shadow.Count)
  Add-Content $Out ("WATCHDOG deferred-for-a-live-hold lines this boot: " + $held.Count)
  if ($t0w) {
    $in10 = 0; $in180 = 0
    foreach ($a in $alarms) {
      $ts = $null
      try { $ts = [datetime]::ParseExact($a.Line.Substring(0,23), 'yyyy-MM-dd HH:mm:ss.fff', $null) } catch { }
      if ($ts) {
        $off = ($ts - $t0w).TotalSeconds
        Add-Content $Out ("  ! +{0:N1}s  {1}" -f $off, $a.Line)
        if ($off -le 10)  { $in10++ }
        if ($off -le 180) { $in180++ }
      } else {
        Add-Content $Out ("  ! (untimed) " + $a.Line)
      }
    }
    Add-Content $Out ("WATCHDOG alarms in the first 10s (install grace - MUST be 0): " + $in10)
    Add-Content $Out ("WATCHDOG alarms in the first 180s (reported, not asserted): " + $in180)
    Add-Content $Out ("WATCHDOG VERDICT no alarm inside the install grace: " + ($in10 -eq 0))
  } else {
    Add-Content $Out "WATCHDOG: boot timestamp did not parse - offsets unavailable"
  }
  # Positive control for this whole block: the hook install line, already
  # counted above. If THAT is present and these are 0, the scan works and the
  # log is genuinely quiet.

  # ---- C. the shown-over line (PROBLEM 243) --------------------------------
  $shown = @($since | Select-String -Pattern 'guide_hud: shown over')
  Add-Content $Out ("SHOWN-OVER 'guide_hud: shown over' lines this boot: " + $shown.Count)
  $shown | Select-Object -First 5 | ForEach-Object { Add-Content $Out ("  + " + $_.Line) }
  Add-Content $Out ("SHOWN-OVER NOTE: this line is written when the ring is SHOWN. It cannot " +
                    "appear until the owner holds Space. 0 on a fresh boot is correct.")

  # ---- D. the first-run tour (PROBLEM 242) ---------------------------------
  # There is no Rust log line for the tour - it is entirely frontend. What CAN
  # be checked from here is the config field it writes, and only AFTER the
  # owner has seen it. Absent is the correct state before the first run.
  if (Test-Path $cfg) {
    $cfgTxt = Get-Content $cfg -Raw
    Add-Content $Out ("TOUR config contains tour_done: " + ($cfgTxt -match '"tour_done"'))
    Add-Content $Out ("TOUR NOTE: absent is CORRECT until the owner has seen the walkthrough. " +
                      "serde default is false, so an absent field means never seen.")
  }
}

# ---------------------------------------------------------------------------
# 1.0.101 ADDITION - the four things this release ships that nothing above
# looks at, plus the config comparison the BTreeMap change forces.
#
# Placed OUTSIDE the `if (Test-Path $log)` block above on purpose: three of
# the four read the filesystem or the config, not the log, and a missing log
# must not silently skip them.
# ---------------------------------------------------------------------------
$log101 = Join-Path $env:APPDATA 'Spaceadom\debug.log'
if (Test-Path $log101) {
  $lines101 = Get-Content $log101
  $bootIdx101 = 0
  for ($k = $lines101.Count - 1; $k -ge 0; $k--) {
    if ($lines101[$k] -match 'logger initialised|Spaceadom v') { $bootIdx101 = $k; break }
  }
  $since101 = @($lines101 | Select-Object -Skip $bootIdx101)

  # A. the OS light/dark watcher (REVIEW FIXES 2026-09-05, frontend/CI lane).
  #    It logs ONCE at startup and again on every change. One line on a fresh
  #    boot is the correct count; more than one means Windows changed theme
  #    while the probe was being taken, which is not a fault.
  $theme = @($since101 | Select-String -Pattern 'theme: watching the Windows app light/dark setting')
  Add-Content $Out ("THEME watcher lines this boot: " + $theme.Count)
  $theme | ForEach-Object { Add-Content $Out ("  + " + $_.Line) }
  Add-Content $Out ("THEME VERDICT watcher started: " + ($theme.Count -ge 1))

  # B. the updater's install-kind verdict. This machine is a per-user NSIS
  #    install, so the ONLY correct answer is Nsis. Msi here would mean the
  #    classifier had adopted an HKLM product for our folder, which is
  #    PROBLEM 244's shape; Unknown would mean uninstall.exe went missing.
  $kind = @($since101 | Select-String -Pattern 'updater: install kind decided')
  Add-Content $Out ("UPDATER kind lines this boot: " + $kind.Count)
  $kind | ForEach-Object { Add-Content $Out ("  + " + $_.Line) }
  Add-Content $Out ("UPDATER VERDICT kind is Nsis: " +
                    (@($kind | Where-Object { $_.Line -match 'Nsis' }).Count -ge 1))

  # C. safe mode must be OFF, and must not have been entered.
  $sm = @($since101 | Select-String -Pattern 'safe-mode')
  Add-Content $Out ("SAFE-MODE lines this boot: " + $sm.Count)
  $sm | ForEach-Object { Add-Content $Out ("  + " + $_.Line) }
  $entered = @($since101 | Select-String -Pattern 'safe-mode-entered-after-three-consecutive-startup-crashes-spaceadom')
  Add-Content $Out ("SAFE-MODE VERDICT entered: " + ($entered.Count -ge 1) + "  (MUST be False)")
}

# D. the boot counter FILE. CLAUDE.md: it must not exist, or must read 0.
$boot = Join-Path $env:APPDATA 'Spaceadom\boot-attempts.json'
if (Test-Path $boot) {
  $bt = (Get-Content $boot -Raw)
  Add-Content $Out ("SAFE-MODE counter file: present - " + $bt.Trim())
  try {
    $bj = $bt | ConvertFrom-Json
    Add-Content $Out ("SAFE-MODE counter failed_starts: " + $bj.failed_starts)
  } catch { Add-Content $Out ("SAFE-MODE counter: could not parse - " + $_.Exception.Message) }
} else {
  Add-Content $Out "SAFE-MODE counter file: ABSENT (which is the clean state)"
}

# E. CONFIG, SEMANTICALLY. `Profile::bindings` became a BTreeMap in the
#    PROBLEM 250 follow-up, so the FIRST save by 1.0.101 rewrites every
#    binding map in sorted key order. The bytes and therefore the SHA-256 will
#    differ, and that difference is CORRECT - it is not data loss and it is
#    not the installer touching the config. A hash comparison alone would read
#    it as damage, so compare the parsed objects instead: sort every object's
#    keys recursively, re-serialise both sides, and compare THOSE.
$preCopy  = 'D:\Claude-Projects\SpaceToggle-V14\_config-live-copy-1.0.107-pre.json'
$postCfg  = Join-Path $env:APPDATA 'Spaceadom\config.json'
function Get-Canonical($o) {
  if ($null -eq $o) { return $null }
  if ($o -is [System.Management.Automation.PSCustomObject]) {
    $h = [ordered]@{}
    foreach ($n in ($o.PSObject.Properties.Name | Sort-Object)) { $h[$n] = Get-Canonical $o.$n }
    return $h
  }
  if ($o -is [System.Collections.IEnumerable] -and -not ($o -is [string])) {
    return @($o | ForEach-Object { Get-Canonical $_ })
  }
  return $o
}
if ((Test-Path $preCopy) -and (Test-Path $postCfg)) {
  try {
    $a = Get-Canonical (Get-Content $preCopy -Raw | ConvertFrom-Json) | ConvertTo-Json -Depth 40 -Compress
    $b = Get-Canonical (Get-Content $postCfg -Raw | ConvertFrom-Json) | ConvertTo-Json -Depth 40 -Compress
    Add-Content $Out ("CONFIG SEMANTIC pre length: " + $a.Length + "  post length: " + $b.Length)
    Add-Content $Out ("CONFIG SEMANTIC VERDICT identical as maps: " + ($a -eq $b))
    if ($a -ne $b) {
      Add-Content $Out ("CONFIG SEMANTIC pre  SHA256: " + (Get-FileHash -InputStream ([IO.MemoryStream]::new([Text.Encoding]::UTF8.GetBytes($a))) -Algorithm SHA256).Hash)
      Add-Content $Out ("CONFIG SEMANTIC post SHA256: " + (Get-FileHash -InputStream ([IO.MemoryStream]::new([Text.Encoding]::UTF8.GetBytes($b))) -Algorithm SHA256).Hash)
    }
  } catch {
    Add-Content $Out ("CONFIG SEMANTIC: could not compare - " + $_.Exception.Message)
  }
} else {
  Add-Content $Out "CONFIG SEMANTIC: one of the two files is missing - no comparison"
}
