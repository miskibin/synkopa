Var WavePreviousAssociation
Var WaveKeepAssociation
Var SynkopaPreviousAssociation
Var SynkopaKeepAssociation
Var WaveOldStartMenu
Var WaveOldDesktop

; Synkopa was called WAVE up to 0.9.21. The installer keys the install folder, the uninstall
; entry and the shortcuts on the product name, so without this an update would leave WAVE
; installed beside Synkopa.
!define WAVE_OLD_UNINSTKEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\WAVE"

; Both project extensions (`.synkopa`, and `.wave` from before the rename) share one class,
; WAVE.Project. Tauri backs up an extension's previous owner on every install, so an in-place
; update would record WAVE.Project as its own predecessor and an uninstall would leave the
; extension pointing at a class that no longer exists. These keep the owner from before the
; first install, and an owner the user chose since.
!macro PROJECT_ASSOC_BEFORE_INSTALL EXT PREVIOUS KEEP
  ReadRegStr ${PREVIOUS} SHCTX "Software\Classes\${EXT}" ""
  StrCpy ${KEEP} 0
  ${If} ${PREVIOUS} == "WAVE.Project"
    ReadRegStr ${PREVIOUS} SHCTX "Software\Classes\${EXT}" "WAVE.Project_backup"
    ; never its own predecessor, whatever an earlier installer recorded
    ${If} ${PREVIOUS} == "WAVE.Project"
      StrCpy ${PREVIOUS} ""
    ${EndIf}
  ${Else}
    ReadRegStr $R0 SHCTX "Software\Classes\WAVE.Project\shell\open\command" ""
    ${If} $R0 != ""
    ${AndIf} ${PREVIOUS} != ""
      StrCpy ${KEEP} 1
    ${EndIf}
  ${EndIf}
!macroend

!macro PROJECT_ASSOC_AFTER_INSTALL EXT PREVIOUS KEEP
  WriteRegStr SHCTX "Software\Classes\${EXT}" "WAVE.Project_backup" "${PREVIOUS}"
  ${If} ${KEEP} == 1
    WriteRegStr SHCTX "Software\Classes\${EXT}" "" "${PREVIOUS}"
  ${EndIf}
!macroend

!macro PROJECT_ASSOC_BEFORE_UNINSTALL EXT CURRENT
  ReadRegStr ${CURRENT} SHCTX "Software\Classes\${EXT}" ""
  ${If} ${CURRENT} != "WAVE.Project"
    WriteRegStr SHCTX "Software\Classes\${EXT}" "WAVE.Project_backup" "${CURRENT}"
  ${EndIf}
!macroend

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

  !insertmacro PROJECT_ASSOC_BEFORE_INSTALL ".wave" $WavePreviousAssociation $WaveKeepAssociation
  !insertmacro PROJECT_ASSOC_BEFORE_INSTALL ".synkopa" $SynkopaPreviousAssociation $SynkopaKeepAssociation
!macroend

!macro NSIS_HOOK_POSTINSTALL
  ; Tauri registers WAVE.Project and copies the icon resource before this hook.
  !insertmacro PROJECT_ASSOC_AFTER_INSTALL ".wave" $WavePreviousAssociation $WaveKeepAssociation
  !insertmacro PROJECT_ASSOC_AFTER_INSTALL ".synkopa" $SynkopaPreviousAssociation $SynkopaKeepAssociation
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
  !insertmacro PROJECT_ASSOC_BEFORE_UNINSTALL ".wave" $WavePreviousAssociation
  !insertmacro PROJECT_ASSOC_BEFORE_UNINSTALL ".synkopa" $SynkopaPreviousAssociation
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  !insertmacro UPDATEFILEASSOC
!macroend
