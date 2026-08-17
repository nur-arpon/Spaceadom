; installer-hooks.nsh — uninstall cleanup for Spaceadom.
;
; PROBLEM 126. Uninstalling Spaceadom left its autostart entries behind.
;
; The app registers a Scheduled Task named "Spaceadom" (and, on machines where
; creating that task is refused, an HKCU Run value instead). Nothing removed
; either of them on uninstall: the code that deletes a stale task runs when the
; APP LAUNCHES, and after an uninstall the app never launches again.
;
; The result is an orphan. Every logon, Windows tries to start an executable
; that is no longer there, fails, and records it in Task Scheduler. No popup,
; no visible damage — just a permanent failing entry on a machine belonging to
; someone who thought they had removed this program. Microsoft Store policy
; 10.2.7 requires a product to "cleanly uninstall and remove" itself.
;
; WHAT IS DELIBERATELY NOT REMOVED: %APPDATA%\Spaceadom (config.json and
; debug.log) and %LOCALAPPDATA%\SpaceadomBackups. Those are the user's own
; profiles and key bindings. Deleting them silently would mean reinstalling
; costs someone every binding they ever set, and an uninstaller is not the
; place to ask. The README says where they live for anyone who wants them gone.
;
; KNOWN GAP: this covers the NSIS installer (Spaceadom_*_x64-setup.exe) only.
; Tauri v2 exposes `installerHooks` for NSIS and has no documented equivalent
; for the WiX/MSI bundler, so the .msi still leaves the task behind. The
; setup.exe is the file handed to users and the one the Store accepts under
; policy 10.2.9, so that is the one that matters — but if the MSI ever becomes
; the primary artifact, this needs a WiX custom action to match.

!macro NSIS_HOOK_PREUNINSTALL
  DetailPrint "Removing the Spaceadom logon entries..."

  ; /F so it does not prompt; failure is fine and expected when the task was
  ; never created (a standard user account falls back to the Run value below).
  nsExec::ExecToLog 'schtasks /Delete /F /TN "Spaceadom"'
  Pop $0

  ; The fallback autostart, used when creating the task was refused.
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Spaceadom"

  ; Legacy identities from before the 1.0.0 rename (PROBLEM 45). Harmless if
  ; absent, and they would otherwise outlive every version that knew about them.
  nsExec::ExecToLog 'schtasks /Delete /F /TN "SpaceToggle OS"'
  Pop $0
  nsExec::ExecToLog 'schtasks /Delete /F /TN "SpaceToggleV14"'
  Pop $0
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "SpaceToggle OS"
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "SpaceToggleV14"
!macroend
