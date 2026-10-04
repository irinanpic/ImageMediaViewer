@echo off
setlocal
title Register ImageMediaViewer Protocol
set "SCRIPT_DIR=%~dp0"
powershell -ExecutionPolicy Bypass -NoProfile -File "%SCRIPT_DIR%register-protocol.ps1"
echo.
pause
endlocal
