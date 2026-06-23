!macro GU_XUAN_YOU_KILL_PROCESS IMAGE_NAME
  DetailPrint "Stopping ${IMAGE_NAME} if it is still running..."
  nsExec::ExecToLog '"$SYSDIR\taskkill.exe" /F /T /IM "${IMAGE_NAME}"'
  Pop $0
!macroend

!macro GU_XUAN_YOU_STOP_RUNNING_PROCESSES
  !insertmacro GU_XUAN_YOU_KILL_PROCESS "${MAINBINARYNAME}.exe"
  Sleep 800
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro GU_XUAN_YOU_STOP_RUNNING_PROCESSES
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro GU_XUAN_YOU_STOP_RUNNING_PROCESSES
!macroend

!macro NSIS_HOOK_POSTINSTALL
  SetShellVarContext current
  ReadEnvStr $0 "LOCALAPPDATA"
  StrCmp $0 "" 0 +2
  StrCpy $0 "$INSTDIR\.."
  StrCpy $1 "$0\股选优"
  CreateDirectory "$SMPROGRAMS\股选优"
  SetOutPath "$1"
  CreateShortCut "$DESKTOP\股选优.lnk" "$WINDIR\explorer.exe" '"$1\${MAINBINARYNAME}.exe"' "$1\${MAINBINARYNAME}.exe" 0
  CreateShortCut "$SMPROGRAMS\股选优\股选优.lnk" "$WINDIR\explorer.exe" '"$1\${MAINBINARYNAME}.exe"' "$1\${MAINBINARYNAME}.exe" 0
!macroend
