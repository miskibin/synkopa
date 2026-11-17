Var WavePreviousAssociation
Var WaveKeepAssociation
Var WaveOldStartMenu
Var WaveOldDesktop

; Synkopa was called WAVE up to 0.9.21. The installer keys the install folder, the uninstall
; entry and the shortcuts on the product name, so without this an update would leave WAVE
; installed beside Synkopa.
!define WAVE_OLD_UNINSTKEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\WAVE"

!macro NSIS_HOOK_PREINSTALL
  ; Remove the WAVE install first (its uninstaller keeps settings, the library and projects:
  ; silent mode never ticks "delete application data"), and remember which shortcuts it had.
  StrCpy $WaveOldStartMenu 0
  StrCpy $WaveOldDesktop 0
  ReadRegStr $R0 SHCTX "${WAVE_OLD_UNINSTKEY}" "UninstallString"
  ReadRegStr $R1 SHCTX "${WAVE_OLD_UNINSTKEY}" "InstallLocation"
  StrCpy $R2 $R1 1
  ${If} $R2 == '"'
    StrCpy $R1 $R1 "" 1
    StrCpy $R1 $R1 -1
  ${EndIf}
  ${If} $R0 != ""
  ${AndIf} $R1 != ""
  ${AndIf} $R1 != $INSTDIR
    ${If} ${FileExists} "$SMPROGRAMS\WAVE.lnk"
      StrCpy $WaveOldStartMenu 1
    ${EndIf}
    ${If} ${FileExists} "$DESKTOP\WAVE.lnk"
      StrCpy $WaveOldDesktop 1
    ${EndIf}
    ; `_?=` runs the uninstaller in place, so ExecWait really waits for it
    ExecWait '$R0 /S _?=$R1'
    Delete "$R1\uninstall.exe"
    RMDir "$R1"
  ${EndIf}

  ; On an in-place update, keep the association from before Synkopa was first installed.
  ReadRegStr $WavePreviousAssociation SHCTX "Software\Classes\.wave" ""
  StrCpy $WaveKeepAssociation 0
  ${If} $WavePreviousAssociation == "WAVE.Project"
    ReadRegStr $WavePreviousAssociation SHCTX "Software\Classes\.wave" "WAVE.Project_backup"
  ${Else}
    ReadRegStr $R0 SHCTX "Software\Classes\WAVE.Project\shell\open\command" ""
    ${If} $R0 != ""
      StrCpy $WaveKeepAssociation 1
    ${EndIf}
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTINSTALL
  ; Tauri registers WAVE.Project and copies the icon resource before this hook.
  WriteRegStr SHCTX "Software\Classes\.wave" "WAVE.Project_backup" "$WavePreviousAssociation"
  ${If} $WaveKeepAssociation == 1
    WriteRegStr SHCTX "Software\Classes\.wave" "" "$WavePreviousAssociation"
  ${EndIf}
  WriteRegStr SHCTX "Software\Classes\WAVE.Project\DefaultIcon" "" '$\"$INSTDIR\wave-project.ico$\",0'
  !insertmacro UPDATEFILEASSOC

  ; An update creates no shortcuts of its own: give back the ones the WAVE install had.
  ${If} $WaveOldStartMenu = 1
  ${AndIfNot} ${FileExists} "$SMPROGRAMS\${PRODUCTNAME}.lnk"
    CreateShortcut "$SMPROGRAMS\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
    !insertmacro SetLnkAppUserModelId "$SMPROGRAMS\${PRODUCTNAME}.lnk"
  ${EndIf}
  ${If} $WaveOldDesktop = 1
  ${AndIfNot} ${FileExists} "$DESKTOP\${PRODUCTNAME}.lnk"
    CreateShortcut "$DESKTOP\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
    !insertmacro SetLnkAppUserModelId "$DESKTOP\${PRODUCTNAME}.lnk"
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ; Preserve any replacement association when Tauri restores the backed-up default.
  ReadRegStr $WavePreviousAssociation SHCTX "Software\Classes\.wave" ""
  ${If} $WavePreviousAssociation != "WAVE.Project"
    WriteRegStr SHCTX "Software\Classes\.wave" "WAVE.Project_backup" "$WavePreviousAssociation"
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  !insertmacro UPDATEFILEASSOC
!macroend
