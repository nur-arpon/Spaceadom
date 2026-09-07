@echo off
REM ===========================================================================
REM postinstall-probe.cmd — after install-real.cmd: is the REAL machine now
REM RUNNING 1.0.107? Launched via explorer.exe (PROBLEM 143). Read-only.
REM ===========================================================================
setlocal
set ROOT=D:\Claude-Projects\SpaceToggle-V14
set OUT=%ROOT%\postinstall-probe.txt

> "%OUT%" echo === postinstall-probe (AFTER 1.0.107 install) ===
>>"%OUT%" echo when: %DATE% %TIME%

powershell -NoProfile -ExecutionPolicy Bypass -File "%ROOT%\scripts\postinstall-probe.ps1" -Out "%OUT%"
>>"%OUT%" echo === probe done ===
endlocal
