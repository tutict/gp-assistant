!macro GP_ASSISTANT_KILL_PROCESS IMAGE_NAME
  DetailPrint "Stopping ${IMAGE_NAME} if it is still running..."
  nsExec::ExecToLog '"$SYSDIR\taskkill.exe" /F /T /IM "${IMAGE_NAME}"'
  Pop $0
!macroend

!macro GP_ASSISTANT_STOP_RUNNING_PROCESSES
  !insertmacro GP_ASSISTANT_KILL_PROCESS "${MAINBINARYNAME}.exe"
  !insertmacro GP_ASSISTANT_KILL_PROCESS "gp-assistant-desktop.exe"
  !insertmacro GP_ASSISTANT_KILL_PROCESS "stock-optimizer-backend.exe"
  !insertmacro GP_ASSISTANT_KILL_PROCESS "gp-assistant-backend.exe"
  !insertmacro GP_ASSISTANT_KILL_PROCESS "app-assistant-backend.exe"
  Sleep 800
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro GP_ASSISTANT_STOP_RUNNING_PROCESSES
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro GP_ASSISTANT_STOP_RUNNING_PROCESSES
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
