@echo off
setlocal EnableExtensions
title FlexDisplay Wi-Fi Safe Helper (No PowerShell)

echo.
echo [INFO] Use START_SAFE.bat and choose Wi-Fi mode.
echo [INFO] Then use your PC IPv4 in the Android app with port 9001.
echo.
ipconfig | findstr /I "IPv4"
echo.
pause
exit /b 0
