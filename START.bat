@echo off
setlocal
set "ROOT=%~dp0"
if exist "%ROOT%FlexDisplay.vbs" (
    wscript.exe //B "%ROOT%FlexDisplay.vbs"
) else (
    powershell.exe -WindowStyle Hidden -ExecutionPolicy Bypass -NoProfile -File "%ROOT%scripts\start-app.ps1"
)
exit /b 0
