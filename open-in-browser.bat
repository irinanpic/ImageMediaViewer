@echo off
setlocal
title ImageMediaViewer Browser Launcher

set "SCRIPT_DIR=%~dp0"
set "BACKEND_EXE=%SCRIPT_DIR%src-tauri\target\debug\image-media-viewer.exe"
set "APP_URL=http://127.0.0.1:14201/"

echo ====================================================
echo        ImageMediaViewer Browser Launcher
echo ====================================================

REM 1. Ensure backend is running
tasklist /FI "IMAGENAME eq image-media-viewer.exe" 2>NUL | find /I /N "image-media-viewer.exe">NUL
if "%ERRORLEVEL%"=="0" (
    echo [OK] Backend server is running on port 14201.
) else (
    echo [*] Starting backend server...
    start "" /B "%BACKEND_EXE%"
    timeout /t 1 /nobreak >nul
)

REM 2. Open in default browser
echo [*] Opening default browser at %APP_URL%
start "" "%APP_URL%"

echo [OK] Opened in browser!
endlocal
exit /b 0
