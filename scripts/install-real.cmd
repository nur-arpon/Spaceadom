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
set SETUP=%ROOT%\src-tauri\target\release\bundle\nsis\Spaceadom_1.0.59_x64-setup.exe

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
set EXE=%LOCALAPPDATA%\Spaceadom\spaceadom.exe
>>"%OUT%" echo installed path: %EXE%
if not exist "%EXE%" (
  >>"%OUT%" echo RESULT: FAIL - exe absent after install
  exit /b 1
)

powershell -NoProfile -Command ^
  "$e='%EXE%';" ^
  "$v=(Get-Item $e).VersionInfo.FileVersion;" ^
  "$t=(Get-Item $e).LastWriteTime;" ^
  "$b=[Text.Encoding]::ASCII.GetString([IO.File]::ReadAllBytes($e));" ^
  "$run=(Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' -Name Spaceadom -EA SilentlyContinue).Spaceadom;" ^
  "Add-Content '%OUT%' ('version: '+$v);" ^
  "Add-Content '%OUT%' ('written: '+$t);" ^
  "Add-Content '%OUT%' ('Run key: '+$run);" ^
  "Add-Content '%OUT%' ('marker spec-card: '+($b -match 'spec-card'));" ^
  "Add-Content '%OUT%' ('marker sld-tail:  '+($b -match 'sld-tail'));" ^
  "Add-Content '%OUT%' ('marker thrustOn:  '+($b -match 'thrustOn'));" ^
  "Add-Content '%OUT%' ('marker Boss Key:  '+($b -match 'Boss Key'))"

REM Start it, so the owner is looking at the build that was just installed.
start "" "%EXE%"
>>"%OUT%" echo RESULT: installed and started
endlocal
