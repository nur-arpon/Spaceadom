<#
  verify-session-end.ps1 — PROBLEM 224.

  Proves, in one pass and without waiting for a real Windows shutdown, whether
  the running Spaceadom survives WM_ENDSESSION or dies in tao's
  "cannot move state from Destroyed" panic.

  ---------------------------------------------------------------------------
  THIS SCRIPT CLOSES THE RUNNING APP. That is the point of it. It is not
  destructive beyond that — no file is written, no setting is touched, and
  Spaceadom can simply be started again — but it WILL take the app down, so it
  never runs without -Run.
  ---------------------------------------------------------------------------

  ESTABLISH THE POSITIVE CONTROL FIRST. Run this against a build that does NOT
  have the fix and confirm it panics with the exact message. A harness that
  cannot produce the failure is not measuring anything, and an "after" taken
  without a matching "before" proves nothing at all — this project's own rule
  (CLAUDE.md: *a check that cannot produce a negative result is not a check*).

    Against 1.0.94 or earlier   ->  EXPECT: process gone, and debug.log ends in
                                    "PANIC on thread 'main' ... cannot move
                                    state from Destroyed"
    With the PROBLEM 224 fix    ->  EXPECT: process gone, and debug.log ends in
                                    "session: WM_ENDSESSION(TRUE)" then
                                    "session: teardown finished — exiting with
                                    code 0", and NO panic line at all.

  Anything else is a VOID run, not a pass.

  Usage (read the note about the sandbox below):
      powershell -ExecutionPolicy Bypass -File scripts\verify-session-end.ps1
      powershell -ExecutionPolicy Bypass -File scripts\verify-session-end.ps1 -Run

  SANDBOX NOTE: an AI agent shell on this machine runs inside an MSIX container
  (CLAUDE.md, PROBLEM 143). `%APPDATA%\Spaceadom\debug.log` does read live from
  there, so the log half of this is trustworthy — but if anything looks
  impossible, re-run it from a real console:
      Start-Process explorer.exe -ArgumentList '<full path to this script>'
#>

param(
    [switch]$Run
)

$ErrorActionPreference = 'Stop'

Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Collections.Generic;
using System.Runtime.InteropServices;

public class SessionEndProbe {
    delegate bool EnumProc(IntPtr h, IntPtr l);

    [DllImport("user32.dll")]
    static extern bool EnumThreadWindows(uint tid, EnumProc cb, IntPtr l);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);

    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    static extern IntPtr SendMessageTimeoutW(
        IntPtr hWnd, uint msg, IntPtr wParam, IntPtr lParam,
        uint flags, uint timeoutMs, out IntPtr result);

    // hwnd, class, title
    public static List<string[]> ThreadWindows(uint tid) {
        var found = new List<string[]>();
        EnumThreadWindows(tid, (h, l) => {
            var c = new StringBuilder(256); GetClassNameW(h, c, 256);
            var t = new StringBuilder(256); GetWindowTextW(h, t, 256);
            found.Add(new string[] { h.ToInt64().ToString(), c.ToString(), t.ToString() });
            return true;
        }, IntPtr.Zero);
        return found;
    }

    // SMTO_ABORTIFHUNG (0x2) | SMTO_NORMAL (0x0). Returns "" on success,
    // otherwise a description — a timeout here is itself a result worth seeing.
    public static string Send(long hwnd, uint msg, long wparam, long lparam, uint timeoutMs) {
        IntPtr res;
        IntPtr ok = SendMessageTimeoutW(new IntPtr(hwnd), msg,
                                        new IntPtr(wparam), new IntPtr(lparam),
                                        0x2, timeoutMs, out res);
        if (ok == IntPtr.Zero) {
            int err = Marshal.GetLastWin32Error();
            return (err == 1460) ? "TIMED OUT (the window never answered)"
                                 : ("failed, Win32 error " + err);
        }
        return "";
    }
}
'@ -Language CSharp

$WM_QUERYENDSESSION  = 0x0011
$WM_ENDSESSION       = 0x0016
$WM_MOUSEMOVE        = 0x0200
$ENDSESSION_CLOSEAPP = 0x00000001   # what msiexec / the Restart Manager sends
$TAO_CLASS           = 'Tao Thread Event Target'

# ---------------------------------------------------------------------------
# 1. Find the app and the windows on its UI thread
# ---------------------------------------------------------------------------
$proc = Get-Process -Name spaceadom -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $proc) {
    Write-Host "Spaceadom is not running. Start it and try again." -ForegroundColor Yellow
    exit 2
}

$uiThread = $null
$windows  = @()
foreach ($t in $proc.Threads) {
    $w = [SessionEndProbe]::ThreadWindows([uint32]$t.Id)
    if ($w.Count -gt 0 -and ($w | Where-Object { $_[1] -eq $TAO_CLASS })) {
        $uiThread = $t.Id
        $windows  = $w
        break
    }
}

Write-Host "Spaceadom pid $($proc.Id)"
if (-not $uiThread) {
    Write-Host "VOID — no thread of this process owns a '$TAO_CLASS' window." -ForegroundColor Red
    Write-Host "       Either this is not a tao/Tauri build, or the enumeration was blocked."
    Write-Host "       Do not read anything into a later result taken from this state."
    exit 3
}
Write-Host "UI thread $uiThread owns $($windows.Count) top-level window(s):"
foreach ($w in $windows) { Write-Host ("    0x{0:X}  class='{1}'  title='{2}'" -f [int64]$w[0], $w[1], $w[2]) }

$taoHwnd = [int64](($windows | Where-Object { $_[1] -eq $TAO_CLASS })[0])
$tauri   = @($windows | Where-Object { $_[1] -eq 'Tauri Window' })
Write-Host ("tao event target: 0x{0:X}   Tauri windows: {1}" -f $taoHwnd, $tauri.Count)

if (-not $Run) {
    Write-Host ""
    Write-Host "DRY RUN — nothing was sent. Re-run with -Run to actually perform the test." -ForegroundColor Cyan
    Write-Host "It will send WM_QUERYENDSESSION then WM_ENDSESSION(TRUE, ENDSESSION_CLOSEAPP)"
    Write-Host "to the tao event target, then one WM_MOUSEMOVE to a Tauri window, and END THE APP."
    exit 0
}

# ---------------------------------------------------------------------------
# 2. Mark the log, then reproduce what msiexec did
# ---------------------------------------------------------------------------
$log = Join-Path $env:APPDATA 'Spaceadom\debug.log'
$before = 0
if (Test-Path $log) { $before = (Get-Item $log).Length }
Write-Host ""
Write-Host "debug.log is $before bytes; everything below that offset is this run."

Write-Host "-> WM_QUERYENDSESSION (lParam ENDSESSION_CLOSEAPP) to the tao event target"
$r = [SessionEndProbe]::Send($taoHwnd, $WM_QUERYENDSESSION, 0, $ENDSESSION_CLOSEAPP, 5000)
if ($r) { Write-Host "   $r" -ForegroundColor Yellow }

Write-Host "-> WM_ENDSESSION (wParam TRUE, lParam ENDSESSION_CLOSEAPP) to the tao event target"
$r = [SessionEndProbe]::Send($taoHwnd, $WM_ENDSESSION, 1, $ENDSESSION_CLOSEAPP, 5000)
if ($r) { Write-Host "   $r" -ForegroundColor Yellow }

Start-Sleep -Milliseconds 800

# The second half of the reproduction: ONE more message to a window tao's
# public_window_callback handles. Before the fix the runner is already
# Destroyed by now, and this is the message that trips the panic. After the
# fix the process is already gone and this simply finds nothing.
if ($tauri.Count -gt 0 -and -not $proc.HasExited) {
    Write-Host "-> WM_MOUSEMOVE to a Tauri window (the message that trips the panic)"
    $r = [SessionEndProbe]::Send([int64]$tauri[0][0], $WM_MOUSEMOVE, 0, 0, 3000)
    if ($r) { Write-Host "   $r" -ForegroundColor Yellow }
} else {
    Write-Host "-> the app is already gone; no second message to send"
}

Start-Sleep -Seconds 3
$proc.Refresh()

# ---------------------------------------------------------------------------
# 3. Verdict, read from the log rather than from the exit alone
# ---------------------------------------------------------------------------
$alive = -not $proc.HasExited
$tail  = ''
if (Test-Path $log) {
    $fs = [System.IO.File]::Open($log, 'Open', 'Read', 'ReadWrite')
    try {
        $null = $fs.Seek($before, 'Begin')
        $sr = New-Object System.IO.StreamReader($fs)
        $tail = $sr.ReadToEnd()
    } finally { $fs.Dispose() }
}

Write-Host ""
Write-Host "--- new debug.log lines -------------------------------------------------"
if ($tail.Trim()) { Write-Host $tail } else { Write-Host "(nothing was logged)" }
Write-Host "-------------------------------------------------------------------------"
Write-Host ("process still running: {0}" -f $alive)

$panicked = $tail -match 'cannot move state from Destroyed'
$guarded  = ($tail -match 'session: WM_ENDSESSION\(TRUE\)') -and ($tail -match 'teardown finished')

Write-Host ""
if ($panicked) {
    Write-Host "RESULT: PANICKED — this build has PROBLEM 224." -ForegroundColor Red
    Write-Host "        Valid as the POSITIVE CONTROL. The harness reproduces the crash,"
    Write-Host "        so a later clean run on a fixed build means something."
} elseif ($guarded) {
    Write-Host "RESULT: GUARDED — WM_ENDSESSION was handled and the app exited cleanly." -ForegroundColor Green
    Write-Host "        Only believe this if the positive control above was seen FIRST"
    Write-Host "        on an unfixed build. Also check the Sentry project: one panic must"
    Write-Host "        now be ZERO events here, and a real crash must be ONE, not four."
} else {
    Write-Host "RESULT: VOID — neither the panic nor the guard appeared in the log." -ForegroundColor Yellow
    Write-Host "        The harness did not reproduce anything, so this run proves NOTHING"
    Write-Host "        in either direction. Do not record it as a pass. Likely causes:"
    Write-Host "        the messages never reached the window (check for TIMED OUT above),"
    Write-Host "        or the running exe is not the build you think it is — compare the"
    Write-Host "        'Spaceadom build — version ...' line at the top of debug.log."
}
