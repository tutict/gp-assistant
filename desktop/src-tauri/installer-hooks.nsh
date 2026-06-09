!macro GP_ASSISTANT_KILL_PROCESS IMAGE_NAME
  DetailPrint "Stopping ${IMAGE_NAME} if it is still running..."
  nsExec::ExecToLog '"$SYSDIR\taskkill.exe" /F /T /IM "${IMAGE_NAME}"'
  Pop $0
!macroend

!macro GP_ASSISTANT_STOP_RUNNING_PROCESSES
  !insertmacro GP_ASSISTANT_KILL_PROCESS "${MAINBINARYNAME}.exe"
  !insertmacro GP_ASSISTANT_KILL_PROCESS "gp-assistant-backend.exe"
  !insertmacro GP_ASSISTANT_KILL_PROCESS "app-assistant-backend.exe"
  !insertmacro GP_ASSISTANT_KILL_PROCESS "GP Assistant.exe"
  Sleep 800
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro GP_ASSISTANT_STOP_RUNNING_PROCESSES
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro GP_ASSISTANT_STOP_RUNNING_PROCESSES
!macroend
