@echo off
REM ===========================================================================
REM install-real.cmd — install THIS build onto the real machine, and prove it.
REM
REM PROBLEM 143: the agent shell runs inside an MSIX container that redirects
REM %LOCALAPPDATA% and virtualises HKCU. `setup.exe /S` run from there installs
REM into the container, and every check made from the same shell then agrees
REM with itself and is wrong. explorer.exe runs OUTSIDE the container, so this
REM script must be launched as:
REM
REM   Start-Process explorer.exe -ArgumentList 'D:\...\scripts\install-real.cmd'
REM
REM It writes its findings to D:\ — a drive the container does not redirect —
REM so the result can be read back and believed.
REM ===========================================================================
setlocal
set ROOT=D:\Claude-Projects\SpaceToggle-V14
set OUT=%ROOT%\install-check.txt
set PROOF=%ROOT%\install-proof.txt
set SETUP=%ROOT%\src-tauri\target\release\bundle\nsis\Spaceadom_1.0.67_x64-setup.exe

> "%OUT%" echo === install-real.cmd ===
>>"%OUT%" echo when: %DATE% %TIME%
>>"%OUT%" echo setup: %SETUP%

if not exist "%SETUP%" (
  >>"%OUT%" echo RESULT: FAIL - installer not found
  exit /b 1
)

REM The NSIS PREINSTALL hook kills a running Spaceadom itself, but an update
REM over a running app is the exact scenario that silently did nothing four
REM times (PROBLEM 127). Belt and braces.
taskkill /IM spaceadom.exe /F >nul 2>&1

"%SETUP%" /S
>>"%OUT%" echo installer exit code: %ERRORLEVEL%   (NEVER trust this alone)

REM --- prove it, from out here ------------------------------------------------
REM FRONTEND markers are NOT searchable in the exe: Tauri v2 compresses the
REM embedded assets, so even `st-hud-glow` tests False in a binary that plainly
REM contains it (measured 2026-08-20; the old CLAUDE.md rule is corrected).
REM The chain below replaces it — the marker is in the BUNDLE, and the exe was
REM linked AFTER that bundle was written.
set EXE=%LOCALAPPDATA%\Spaceadom\spaceadom.exe
>>"%OUT%" echo installed path: %EXE%
if not exist "%EXE%" (
  >>"%OUT%" echo RESULT: FAIL - exe absent after install
  exit /b 1
)

REM The proof step writes its OWN file, which is then folded in here. Two
REM earlier shapes of this line both failed, and both failed SILENTLY:
REM   1. no -ExecutionPolicy: powershell refused the script and printed why to
REM      a console nobody was reading, so the output file just stopped.
REM   2. `>>"%OUT%" 2>&1` on this line: cmd holds %OUT% open for the whole
REM      call, so every Add-Content inside the script hit "being used by
REM      another process" and the file gained nothing but errors.
REM Separate files, then concatenate. A verification that can fail silently is
REM not a verification.
powershell -NoProfile -ExecutionPolicy Bypass -File "%ROOT%\scripts\install-proof.ps1" -Exe "%EXE%" -Root "%ROOT%" -Out "%PROOF%" 2>&1
if exist "%PROOF%" (type "%PROOF%" >>"%OUT%") else (>>"%OUT%" echo PROOF STEP PRODUCED NOTHING - powershell never ran)
del "%PROOF%" >nul 2>&1

REM Start it, so the owner is looking at the build that was just installed.
start "" "%EXE%"
>>"%OUT%" echo RESULT: installed and started
endlocal
