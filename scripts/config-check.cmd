@echo off
REM ===========================================================================
REM config-check.cmd — read the owner's LIVE config.json + debug.log from
REM OUTSIDE the agent's MSIX container (PROBLEM 143), and copy the config to a
REM D: path so it can be parsed without reading a shadow.
REM
REM Launch as:
REM   Start-Process explorer.exe -ArgumentList 'D:\...\scripts\config-check.cmd'
REM
REM READ ONLY. Nothing here writes to config.json — it is live and in use.
REM ===========================================================================
setlocal
set ROOT=D:\Claude-Projects\SpaceToggle-V14
set OUT=%ROOT%\config-check.txt
set COPY=%ROOT%\_config-live-copy.json

> "%OUT%" echo === config-check (live, read-only) ===
>>"%OUT%" echo when: %DATE% %TIME%

powershell -NoProfile -ExecutionPolicy Bypass -File "%ROOT%\scripts\config-check.ps1" -Out "%OUT%" -Copy "%COPY%"
>>"%OUT%" echo === config-check done ===
endlocal
