@echo off
setlocal
cd /d "%~dp0"

REM Check if backend server (image-media-viewer.exe) is already running
tasklist /FI "IMAGENAME eq image-media-viewer.exe" 2>nul | find /i "image-media-viewer.exe" >nul
if "%ERRORLEVEL%"=="0" (
    echo [ImageMediaViewer] Server is already running.
    exit /b 0
)

set "EXE_PATH="
if exist "%~dp0src-tauri\target\release\image-media-viewer.exe" (
    set "EXE_PATH=%~dp0src-tauri\target\release\image-media-viewer.exe"
    set "BUILD_TYPE=[release]"
) else if exist "%~dp0src-tauri\target\debug\image-media-viewer.exe" (
    set "EXE_PATH=%~dp0src-tauri\target\debug\image-media-viewer.exe"
    set "BUILD_TYPE=[debug]"
) else if exist "%~dp0image-media-viewer.exe" (
    set "EXE_PATH=%~dp0image-media-viewer.exe"
    set "BUILD_TYPE="
)

if "%EXE_PATH%"=="" (
    echo [ERROR] image-media-viewer.exe not found. Please build the project first.
    exit /b 1
)

REM Launch truly independent background process via WMI (decoupled from caller job objects)
powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -Command "([wmiclass]'Win32_Process').Create('\"%EXE_PATH%\" --server-only', '%~dp0') | Out-Null"
echo [ImageMediaViewer] Server started %BUILD_TYPE%.
exit /b 0