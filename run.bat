@echo off
setlocal
title ImageMediaViewer Desktop Launcher

set "SCRIPT_DIR=%~dp0"
powershell -ExecutionPolicy Bypass -NoProfile -File "%SCRIPT_DIR%run.ps1" %*
endlocal
