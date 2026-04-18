@echo off
setlocal EnableExtensions
title FlexDisplay USB Safe Helper (No PowerShell)

set "ADB=adb"
where adb >nul 2>nul
if errorlevel 1 (
  if exist "%~dp0.runtime\adb\platform-tools\adb.exe" set "ADB=%~dp0.runtime\adb\platform-tools\adb.exe"
)

"%ADB%" version >nul 2>nul
if errorlevel 1 (
  echo [ERROR] ADB not found.
  echo Install Android platform-tools or place adb in PATH.
  pause
  exit /b 1
)

echo [INFO] Using ADB: %ADB%
"%ADB%" devices
"%ADB%" reverse --remove-all
"%ADB%" reverse tcp:9001 tcp:9001
"%ADB%" shell monkey -p com.example.tabletmonitor -c android.intent.category.LAUNCHER 1
echo [OK] USB reverse configured. Now run START_SAFE.bat in USB mode.
pause
exit /b 0
