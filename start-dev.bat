@echo off
setlocal EnableExtensions EnableDelayedExpansion
cd /d "%~dp0"

set "START_TIME=%DATE% %TIME%"
set "PS_SCRIPT=%~dp0scripts\start-dev.ps1"
set "POWERSHELL_EXE=%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe"
if not exist "%POWERSHELL_EXE%" set "POWERSHELL_EXE=powershell.exe"

echo.
echo [gp-dev] ============================================================
echo [gp-dev] GP Assistant one-click dev launcher
echo [gp-dev] ============================================================
echo [gp-dev] Start time: %START_TIME%
echo [gp-dev] Repo: %CD%
echo [gp-dev] Batch: %~f0
echo [gp-dev] PowerShell script: %PS_SCRIPT%
echo [gp-dev] Raw args: %*
echo.
echo [gp-dev] Default mode: Tauri desktop. Use "start-dev.bat web" for Python Web on port 8010.
echo [gp-dev] Logs are written to logs\dev\*-dev-*.log and mirrored to latest aliases.
echo.

if not exist "%PS_SCRIPT%" (
  echo [gp-dev] ERROR: PowerShell script was not found.
  echo [gp-dev] Missing: %PS_SCRIPT%
  pause
  exit /b 2
)

echo [gp-dev] Launching PowerShell command:
echo [gp-dev]   "%POWERSHELL_EXE%" -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%PS_SCRIPT%" %*
echo.

"%POWERSHELL_EXE%" -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%PS_SCRIPT%" %*
set "EXIT_CODE=%ERRORLEVEL%"

echo.
echo [gp-dev] End time: %DATE% %TIME%
echo [gp-dev] scripts\start-dev.ps1 exited with code %EXIT_CODE%.

if "%EXIT_CODE%"=="0" exit /b 0

echo.
echo [gp-dev] Startup failed. Review the log above and the latest log under logs\dev.
echo [gp-dev] Useful retries:
echo [gp-dev]   start-dev.bat
echo [gp-dev]   start-dev.bat -PreflightOnly -NoTranscript
echo [gp-dev]   start-dev.bat web
echo [gp-dev]   start-dev.bat web -PreflightOnly -NoTranscript
echo [gp-dev]   start-dev.bat web -NoBuild -NoKill
echo.
pause
exit /b %EXIT_CODE%
