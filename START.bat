@echo off
setlocal EnableDelayedExpansion
title FlexDisplay

echo.
echo  FlexDisplay - double-click START to connect your tablet.
echo  First run downloads ADB and FFmpeg automatically.
echo.

set "LAUNCHER=%~dp0scripts\launcher.ps1"

if not exist "%LAUNCHER%" (
    echo  ERROR: scripts\launcher.ps1 not found
    echo  Note: Make sure you run this from the project root.
    pause
    exit /b 1
)

:: Launch PowerShell with the unified launcher (bypass execution policy for this script only)
powershell -ExecutionPolicy Bypass -NoProfile -File "%LAUNCHER%"

:: Preserve exit code
exit /b %ERRORLEVEL%
