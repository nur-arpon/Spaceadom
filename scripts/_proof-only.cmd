@echo off
REM _proof-only.cmd — re-run install-proof.ps1 alone, outside the agent's MSIX
REM container (PROBLEM 143), without re-installing anything. Created by the
REM 1.0.103 ship agent because the proof step is RE-RUNNABLE by design: the
REM law-6 own-window block needs the owner to hold Space, which happens after
REM the install. Launch with:
REM   Start-Process explorer.exe -ArgumentList '...\scripts\_proof-only.cmd'
setlocal
set ROOT=D:\Claude-Projects\SpaceToggle-V14
set PROOF=%ROOT%\install-proof.txt
set EXE=%LOCALAPPDATA%\Spaceadom\spaceadom.exe
del "%PROOF%" >nul 2>&1
powershell -NoProfile -ExecutionPolicy Bypass -File "%ROOT%\scripts\install-proof.ps1" -Exe "%EXE%" -Root "%ROOT%" -Out "%PROOF%"
>>"%PROOF%" echo === proof done ===
endlocal
