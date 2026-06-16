@echo off
setlocal EnableDelayedExpansion
title FlexDisplay - Stop

set "STOP_SCRIPT=%~dp0scripts\stop-usb.ps1"

if not exist "%STOP_SCRIPT%" (
    echo  ERROR: scripts\stop-usb.ps1 not found
    pause
    exit /b 1
)

powershell -ExecutionPolicy Bypass -NoProfile -File "%STOP_SCRIPT%"
exit /b %ERRORLEVEL%
