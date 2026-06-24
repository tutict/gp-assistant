@echo off
setlocal EnableExtensions EnableDelayedExpansion
cd /d "%~dp0"

set "START_TIME=%DATE% %TIME%"
set "PS_SCRIPT=%~dp0scripts\start-tauri-dev.ps1"
set "POWERSHELL_EXE=%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe"
if not exist "%POWERSHELL_EXE%" set "POWERSHELL_EXE=powershell.exe"

echo.
echo [gp-tauri] ============================================================
echo [gp-tauri] GP Assistant Tauri desktop dev launcher
echo [gp-tauri] ============================================================
echo [gp-tauri] Start time: %START_TIME%
echo [gp-tauri] Repo: %CD%
echo [gp-tauri] Batch: %~f0
echo [gp-tauri] PowerShell script: %PS_SCRIPT%
echo [gp-tauri] Raw args: %*
echo [gp-tauri] PowerShell exe: %POWERSHELL_EXE%
echo.
echo [gp-tauri] Tip: this launcher verifies the desktop/Tauri path, not the Python web server path.
echo [gp-tauri] Tip: default launch runs preflight and starts Tauri in no-watch mode for cleaner logs.
echo [gp-tauri] Tip: the window stays open while tauri dev is running; that is normal, not a stuck loop.
echo [gp-tauri] Tip: session logs are written to logs\dev\tauri-dev-*.log and mirrored to logs\dev\tauri-dev.latest.log.
echo.

if not exist "%PS_SCRIPT%" (
  echo [gp-tauri] ERROR: PowerShell script was not found.
  echo [gp-tauri] Missing: %PS_SCRIPT%
  pause
  exit /b 2
)

set "DEFAULT_ARGS=-RunPreflight -NoWatch"
set "FORWARD_ARGS=%*"
if "%~1"=="" set "FORWARD_ARGS=%DEFAULT_ARGS%"

echo [gp-tauri] Launching PowerShell command:
echo [gp-tauri]   "%POWERSHELL_EXE%" -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%PS_SCRIPT%" !FORWARD_ARGS!
echo.

"%POWERSHELL_EXE%" -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%PS_SCRIPT%" !FORWARD_ARGS!
set "EXIT_CODE=%ERRORLEVEL%"

echo.
echo [gp-tauri] End time: %DATE% %TIME%
echo [gp-tauri] start-tauri-dev.ps1 exited with code %EXIT_CODE%.

if "%EXIT_CODE%"=="0" exit /b 0

echo.
echo [gp-tauri] Tauri desktop startup failed. Review the log above.
echo [gp-tauri] Latest session log: %~dp0logs\dev\tauri-dev.latest.log
echo [gp-tauri] Useful retries:
echo [gp-tauri]   start-tauri-dev.bat -PreflightOnly -NoTranscript
echo [gp-tauri]   start-tauri-dev.bat -RunPreflight -NoWatch -NoTranscript
echo [gp-tauri]   start-tauri-dev.bat -RunPreflight -SkipPrepare -SkipCargoCheck -NoWatch -NoTranscript
echo [gp-tauri]   start-tauri-dev.bat -VerboseTauri -NoWatch -NoTranscript
echo.
pause
exit /b %EXIT_CODE%
