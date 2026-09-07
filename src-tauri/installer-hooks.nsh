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
; UPDATE (PROBLEM 247): %APPDATA%\Spaceadom and %LOCALAPPDATA%\SpaceadomBackups
; are no longer unconditionally kept. An interactive uninstall now ASKS —
; see NSIS_HOOK_PREUNINSTALL / NSIS_HOOK_POSTUNINSTALL below for the question
; and PROBLEM 247 in V14_FIXES_AND_CODE.md for the full reasoning. The default
; answer is still Keep, and a silent or self-update uninstall can never be
; asked, so it always keeps — this only changes what happens when someone
; uninstalls by hand and says No.
;
; KNOWN GAP: this covers the NSIS installer (Spaceadom_*_x64-setup.exe) only.
; Tauri v2 exposes `installerHooks` for NSIS and has no documented equivalent
; for the WiX/MSI bundler, so the .msi still leaves the task behind. The
; setup.exe is the file handed to users and the one the Store accepts under
; policy 10.2.9, so that is the one that matters — but if the MSI ever becomes
; the primary artifact, this needs a WiX custom action to match.

; ---------------------------------------------------------------------------
; PROBLEM 127 — an update could report success and install nothing.
;
; Spaceadom starts with Windows, so it is ALWAYS running when someone installs
; an update. A running process holds its own .exe open, so the installer cannot
; replace it. Windows' Restart Manager notices and asks:
;
;     "Some files that need to be updated are currently in use.
;      The following applications are using files that need to be updated
;      by this setup: Spaceadom"
;
; Answer that dialog and the upgrade works — verified on 2026-08-17, 1.0.35 to
; 1.0.37, confirmed by version stamp AND by content marker.
;
; But in a SILENT install there is nobody to answer it. The dialog cannot be
; shown, so the replacement is deferred to the next reboot and the installer
; exits 0. The user is told it worked and keeps the old version. That is
; exactly what happened here twice: MsiInstaller logged "installed the product
; ... 1.0.37 ... status: 0" at 15:24:03 while Program Files still held 1.0.35.
;
; And silent is not an edge case — Microsoft Store policy 10.2.9 REQUIRES it:
; "Initiating the install must not display an installation user interface
; (i.e., silent install is required), however a User Account Control (UAC)
; dialog is allowed."
;
; So: close the app ourselves, before the files are touched, instead of asking
; a question nobody will hear. Safe to kill — config.json is written on every
; change, never held for later, so nothing is lost.
; ---------------------------------------------------------------------------
; PROBLEM 247's answer, set in NSIS_HOOK_PREUNINSTALL and acted on in
; NSIS_HOOK_POSTUNINSTALL — declared here, at file scope, because this whole
; file is !include'd once near the top of the generated script (ahead of every
; Section), the same place Tauri's own template declares $UpdateMode and
; $PassiveMode.
Var ST_KeepData

!macro NSIS_HOOK_PREINSTALL
  DetailPrint "Closing Spaceadom so its files can be replaced..."
  ; /T kills child processes too (the WebView2 hosts), which hold DLLs open.
  nsExec::Exec 'taskkill /F /T /IM spaceadom.exe'
  Pop $0
  ; Give Windows a moment to release the handles before the copy starts.
  Sleep 1500
!macroend

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

  ; ---------------------------------------------------------------------
  ; PROBLEM 247 — ask whether to keep profiles, key bindings and backups.
  ;
  ; Everything above this point (the scheduled task and both Run values,
  ; current identity and legacy) is removed UNCONDITIONALLY, on every
  ; uninstall, silent or not, update or not — that is autostart cleanup, not
  ; user data, and Store policy 10.2.7 requires it regardless of the answer
  ; below.
  ;
  ; What follows decides only the fate of %APPDATA%\Spaceadom (config.json,
  ; debug.log/.0/.1, picker-cache.json, last-run-version.txt) and
  ; %LOCALAPPDATA%\SpaceadomBackups (the rolling config backups from
  ; PROBLEM 94) — the actual RMDir happens in NSIS_HOOK_POSTUNINSTALL, after
  ; Tauri's own uninstall steps, using the answer captured here in
  ; $ST_KeepData. Two cases must NEVER prompt and must NEVER delete:
  ;
  ;   1. A SILENT uninstall (/S). There is nobody to answer a dialog, and
  ;      Store policy 10.2.9 requires silent to be possible at all — the same
  ;      shape of problem PROBLEM 127 solved for installing.
  ;   2. A SELF-UPDATE. `updater.rs` installs the next version with
  ;      `setup.exe /S /UPDATE /R /ARGS --autostart`; Tauri's generated
  ;      installer.nsi runs that flow through the OLD version's uninstall.exe
  ;      first (its `un.onInit` reads the `/UPDATE` flag into $UpdateMode,
  ;      which the same generated file also uses to skip removing shortcuts
  ;      and the Run-key entry on this exact path). An update is silent by
  ;      construction anyway, but $UpdateMode is checked on its own so this
  ;      can never fire even if that ever changes.
  ;
  ; Default is Keep in every case: $ST_KeepData starts "1" and only a typed
  ; No changes it, matching the dialog's own default button (IDYES).
  ; ---------------------------------------------------------------------
  StrCpy $ST_KeepData 1

  ${If} $UpdateMode = 1
    Goto st247_decided
  ${EndIf}

  IfSilent st247_decided

  MessageBox MB_YESNO|MB_ICONQUESTION "Keep your settings? (profiles, key bindings, backups)$\r$\n$\r$\nChoose No to also delete $APPDATA\Spaceadom and $LOCALAPPDATA\SpaceadomBackups from this PC." IDYES st247_decided
  StrCpy $ST_KeepData 0

  st247_decided:
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; ---------------------------------------------------------------------
  ; PROBLEM 247 continued — act on the answer captured above, after Tauri's
  ; own uninstall section has already removed the app's own install folder,
  ; shortcuts and registry keys. $ST_KeepData is "1" (keep) on every path
  ; that cannot prompt (silent, or an update — see NSIS_HOOK_PREUNINSTALL)
  ; and on every explicit Yes; it is "0" only after an interactive uninstall
  ; where the owner typed No.
  ;
  ; Exactly these two paths, hardcoded, no wildcards, and guarded against an
  ; empty environment variable so a lookup failure can never widen to "the
  ; current directory": nothing outside them is ever touched.
  ;   %APPDATA%\Spaceadom             config.json, debug.log(.0/.1),
  ;                                    picker-cache.json, last-run-version.txt
  ;   %LOCALAPPDATA%\SpaceadomBackups rolling config backups (PROBLEM 94)
  ; ---------------------------------------------------------------------
  ${If} $ST_KeepData = 1
    Goto st247_data_done
  ${EndIf}

  ; Per-user install (nsis.installMode = currentUser): always the current
  ; user's own profile, matching the SetShellVarContext Tauri's own
  ; delete-app-data branch already uses a few lines above in this Section.
  SetShellVarContext current

  DetailPrint "Removing your Spaceadom settings..."
  ${If} $APPDATA != ""
    RMDir /r "$APPDATA\Spaceadom"
  ${EndIf}
  ${If} $LOCALAPPDATA != ""
    RMDir /r "$LOCALAPPDATA\SpaceadomBackups"
  ${EndIf}

  st247_data_done:
!macroend
