@echo off
@chcp 65001 >nul
setlocal
cd /d "%~dp0"

REM Check if backend server (image-media-viewer.exe) is already running
tasklist /FI "IMAGENAME eq image-media-viewer.exe" 2>nul | find /i "image-media-viewer.exe" >nul
if "%ERRORLEVEL%"=="0" (
    echo [ImageMediaViewer] Server is already running.
    exit /b 0
)

REM Check binary and launch in background without opening client window
if exist "src-tauri\target\debug\image-media-viewer.exe" (
    start "" /B "src-tauri\target\debug\image-media-viewer.exe"
    echo [ImageMediaViewer] Server started (debug binary).
    exit /b 0
)

if exist "src-tauri\target\release\image-media-viewer.exe" (
    start "" /B "src-tauri\target\release\image-media-viewer.exe"
    echo [ImageMediaViewer] Server started (release binary).
    exit /b 0
)

if exist "image-media-viewer.exe" (
    start "" /B "image-media-viewer.exe"
    echo [ImageMediaViewer] Server started.
    exit /b 0
)

echo [ERROR] image-media-viewer.exe not found. Please build the project first.
exit /b 1
