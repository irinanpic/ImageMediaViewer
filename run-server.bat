@echo off
setlocal
title ImageMediaViewer Desktop Launcher
chcp 65001 >nul

set "SCRIPT_DIR=%~dp0"
powershell -ExecutionPolicy Bypass -NoProfile -File "%SCRIPT_DIR%run.ps1" %*
endlocal
