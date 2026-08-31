@echo off
REM ===========================================================================
REM preinstall-probe.cmd — read the REAL machine's CURRENT install, before the
REM 1.0.90 setup runs. Launched via explorer.exe so it lives outside the
REM agent's MSIX container (PROBLEM 143). Installs nothing, changes nothing.
REM ===========================================================================
setlocal
set ROOT=D:\Claude-Projects\SpaceToggle-V14
set OUT=%ROOT%\preinstall-probe.txt

> "%OUT%" echo === preinstall-probe (baseline BEFORE 1.0.95) ===
>>"%OUT%" echo when: %DATE% %TIME%

powershell -NoProfile -ExecutionPolicy Bypass -File "%ROOT%\scripts\preinstall-probe.ps1" -Out "%OUT%"
>>"%OUT%" echo === probe done ===
endlocal
