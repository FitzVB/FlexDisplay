@echo off
setlocal
title FlexDisplay - Developer Setup

set "BOOTSTRAP=%~dp0first-run.ps1"

if not exist "%BOOTSTRAP%" (
    echo [ERROR] scripts\first-run.ps1 not found
    pause
    exit /b 1
)

powershell -ExecutionPolicy Bypass -NoProfile -File "%BOOTSTRAP%" -AutoLaunch
exit /b %ERRORLEVEL%
