@echo off
setlocal EnableExtensions EnableDelayedExpansion
cd /d "%~dp0"

echo.
echo [gp-test] Running Tauri/Rust preflight. Python/FastAPI smoke server is retired.
echo.

powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%~dp0scriptsstart-test.ps1" %*
set EXIT_CODE=%ERRORLEVEL%

if not "%EXIT_CODE%"=="0" (
  echo.
  echo Tauri preflight failed with exit code %EXIT_CODE%.
  pause
)

exit /b %EXIT_CODE%
