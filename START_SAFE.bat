@echo off
setlocal EnableExtensions
title FlexDisplay Safe Start (No PowerShell)

set "ROOT=%~dp0"
set "HOST=%ROOT%host-windows.exe"
if not exist "%HOST%" set "HOST=%ROOT%host-windows\target\release\host-windows.exe"

if not exist "%HOST%" (
  echo [ERROR] host-windows executable not found.
  echo Build first or use packaged release that includes host-windows.exe.
  pause
  exit /b 1
)

echo.
echo FlexDisplay Safe Start (No PowerShell)
echo ======================================
echo 1^) USB mode ^(manual ADB^)
echo 2^) Wi-Fi mode
set /p MODE=Choose mode [1/2]:

if "%MODE%"=="2" (
  set "TABLET_MONITOR_LISTEN=0.0.0.0"
  echo [INFO] Wi-Fi mode selected.
  echo [INFO] In Android app, use your PC LAN IP and port 9001.
) else (
  set "TABLET_MONITOR_LISTEN=127.0.0.1"
  echo [INFO] USB mode selected.
  echo [INFO] If needed, run these commands manually before opening app:
  echo        adb reverse --remove-all
  echo        adb reverse tcp:9001 tcp:9001
  echo        adb shell monkey -p com.example.tabletmonitor -c android.intent.category.LAUNCHER 1
)

set "TABLET_MONITOR_FPS=60"
echo [INFO] Starting host...
"%HOST%"
exit /b %ERRORLEVEL%
