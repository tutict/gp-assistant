@echo off
setlocal EnableExtensions EnableDelayedExpansion
cd /d "%~dp0"

set "START_TIME=%DATE% %TIME%"
set "PS_SCRIPT=%~dp0scripts\start-dev.ps1"
set "POWERSHELL_EXE=%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe"
if not exist "%POWERSHELL_EXE%" set "POWERSHELL_EXE=powershell.exe"

REM One-click foreground dev launcher. Backend logs and tracebacks stream
REM live in this window so you can watch for errors. Press Ctrl+C to stop.
REM Hot reload is enabled by default. Pass extra args through, e.g.:  start-dev.bat -Open -LogLevel debug

echo.
echo [gp-dev] ============================================================
echo [gp-dev] GP Assistant foreground dev launcher
echo [gp-dev] ============================================================
echo [gp-dev] Start time: %START_TIME%
echo [gp-dev] Repo: %CD%
echo [gp-dev] Batch: %~f0
echo [gp-dev] PowerShell script: %PS_SCRIPT%
echo [gp-dev] Args: %*
echo [gp-dev] PowerShell exe: %POWERSHELL_EXE%
echo [gp-dev] User: %USERNAME%@%COMPUTERNAME%
echo [gp-dev] OS: %OS%
echo.
echo [gp-dev] Environment overrides:
call :ShowEnv GP_ASSISTANT_PYTHON
call :ShowEnv STOCK_PROVIDER
call :ShowEnv GP_CAPITAL_CACHE
call :ShowEnv GP_CAPITAL_ENABLE_EXTERNAL
call :ShowEnv GP_CAPITAL_LHB_FAST_TIMEOUT
call :ShowEnvState HTTP_PROXY
call :ShowEnvState HTTPS_PROXY
echo.
echo [gp-dev] PATH probes:
call :WhereFirst python
call :WhereFirst py
call :WhereFirst node
call :WhereFirst git
echo.
echo [gp-dev] Tip: hot reload is enabled by default; use -NoReload for single-process debugging.
echo [gp-dev] Tip: use -LogLevel debug for request/import detail, -Port 8011 if 8010 is busy.
echo [gp-dev] Tip: use -DiagnosticsOnly to print environment checks without starting the server.
echo [gp-dev] Tip: use -SkipSyntaxCheck only when you need to bypass startup checks temporarily.
echo.

if not exist "%PS_SCRIPT%" (
  echo [gp-dev] ERROR: PowerShell script was not found.
  echo [gp-dev] Missing: %PS_SCRIPT%
  echo [gp-dev] Current directory: %CD%
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
echo [gp-dev] start-dev.ps1 exited with code %EXIT_CODE%.

if "%EXIT_CODE%"=="0" exit /b 0

echo.
echo [gp-dev] Dev server failed. Context above should include the failing step.
if "%EXIT_CODE%"=="9009" (
  echo [gp-dev] ERROR: Windows could not find one of the commands above.
  echo [gp-dev] Check PowerShell, Python, Node, and PATH probe output.
)
echo [gp-dev] Useful retries:
echo [gp-dev]   start-dev.bat -DiagnosticsOnly -LogLevel debug
echo [gp-dev]   start-dev.bat -LogLevel debug
echo [gp-dev]   start-dev.bat -NoReload -LogLevel debug
echo [gp-dev]   start-dev.bat -Port 8011 -LogLevel debug
echo [gp-dev]   set GP_CAPITAL_ENABLE_EXTERNAL=false ^&^& start-dev.bat -LogLevel debug
echo [gp-dev]   start-dev.bat -SkipSyntaxCheck -LogLevel debug
echo.
pause
exit /b %EXIT_CODE%

:ShowEnv
set "VALUE="
call set "VALUE=%%%~1%%"
if defined VALUE (
  echo [gp-dev]   %~1=!VALUE!
) else (
  echo [gp-dev]   %~1=-
)
set "VALUE="
exit /b 0

:ShowEnvState
set "VALUE="
call set "VALUE=%%%~1%%"
if defined VALUE (
  echo [gp-dev]   %~1=set
) else (
  echo [gp-dev]   %~1=-
)
set "VALUE="
exit /b 0

:WhereFirst
set "FOUND="
for /f "delims=" %%P in ('where %~1 2^>nul') do (
  if not defined FOUND set "FOUND=%%P"
)
if defined FOUND (
  echo [gp-dev]   %~1=!FOUND!
) else (
  echo [gp-dev]   %~1=not found
)
set "FOUND="
exit /b 0
