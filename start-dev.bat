@echo off
setlocal EnableExtensions EnableDelayedExpansion
cd /d "%~dp0"

REM Tauri is now the default development path. Keep this wrapper so existing
REM shortcuts that call start-dev.bat continue to work without Python.

echo.
echo [gp-dev] ============================================================
echo [gp-dev] GP Assistant Tauri desktop dev launcher
echo [gp-dev] ============================================================
echo [gp-dev] start-dev.bat now delegates to start-tauri-dev.bat.
echo [gp-dev] Python/FastAPI is no longer required for the default dev path.
echo.

call "%~dp0start-tauri-dev.bat" %*
exit /b %ERRORLEVEL%
