@echo off
setlocal
net session >nul 2>&1
if %errorlevel% neq 0 (
    powershell.exe -NoProfile -ExecutionPolicy Bypass -Command "Start-Process powershell.exe -Verb RunAs -ArgumentList '-NoProfile -ExecutionPolicy Bypass -File \"\"%~dp0scripts\add-defender-exclusion.ps1\"\" -RootPath \"\"%~dp0\"\"'"
    exit /b 0
)
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\add-defender-exclusion.ps1" -RootPath "%~dp0"
exit /b %errorlevel%
