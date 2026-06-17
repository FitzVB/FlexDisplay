#Requires -Version 5.1
<#!
.SYNOPSIS
Silent desktop launcher — no console window. Opens the host control app in Edge/Chrome app mode.
#>
param(
    [ValidateSet('Usb', 'WiFi')]
    [string]$Mode = 'Usb'
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$logDir = Join-Path $root 'logs'
New-Item -ItemType Directory -Force -Path $logDir | Out-Null
$logFile = Join-Path $logDir 'flexdisplay-start.log'
Start-Transcript -Path $logFile -Append -ErrorAction SilentlyContinue | Out-Null

$ensureRuntimeScript = Join-Path $PSScriptRoot 'ensure-runtime.ps1'
if (Test-Path $ensureRuntimeScript) {
    try {
        & $ensureRuntimeScript -RootPath $root -EnsureAdb -EnsureFfmpeg
    }
    catch {
        Add-Content -Path $logFile -Value "[WARN] Runtime prepare failed: $($_.Exception.Message)"
    }
}

$env:FLEXDISPLAY_EXIT_ON_GUI_CLOSE = '1'
$env:FLEXDISPLAY_FPS = '60'

if ($Mode -eq 'WiFi') {
    Remove-Item Env:FLEXDISPLAY_LISTEN -ErrorAction SilentlyContinue
    & (Join-Path $PSScriptRoot 'start-wifi.ps1') -Silent
}
else {
    $env:FLEXDISPLAY_LISTEN = '127.0.0.1'
    & (Join-Path $PSScriptRoot 'start-usb.ps1') -Silent
}

Stop-Transcript -ErrorAction SilentlyContinue | Out-Null
