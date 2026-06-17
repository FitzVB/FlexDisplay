@echo off
setlocal
set "ROOT=%~dp0"
if exist "%ROOT%FlexDisplay-WiFi.vbs" (
    wscript.exe //B "%ROOT%FlexDisplay-WiFi.vbs"
) else (
    powershell.exe -WindowStyle Hidden -ExecutionPolicy Bypass -NoProfile -File "%ROOT%scripts\start-app.ps1" -Mode WiFi
)
exit /b 0
