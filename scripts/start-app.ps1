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
. (Join-Path $PSScriptRoot 'lib\Common.ps1')

$logDir = Join-Path $root 'logs'
New-Item -ItemType Directory -Force -Path $logDir | Out-Null
$launcherLog = Join-Path $logDir 'flexdisplay-launcher.log'
Start-Transcript -Path $launcherLog -Append -ErrorAction SilentlyContinue | Out-Null

$ensureRuntimeScript = Join-Path $PSScriptRoot 'ensure-runtime.ps1'
if (Test-Path $ensureRuntimeScript) {
    try {
        & $ensureRuntimeScript -RootPath $root -EnsureAdb -EnsureFfmpeg
    }
    catch {
        Add-Content -Path $launcherLog -Value "[WARN] Runtime prepare failed: $($_.Exception.Message)" -ErrorAction SilentlyContinue
    }
}

Stop-Transcript -ErrorAction SilentlyContinue | Out-Null

$env:FLEXDISPLAY_FPS = '60'
$port = 9001

$ErrorActionPreference = 'Continue'

if ($Mode -eq 'WiFi') {
    Remove-Item Env:FLEXDISPLAY_LISTEN -ErrorAction SilentlyContinue
    & (Join-Path $PSScriptRoot 'start-wifi.ps1') -Silent
}
else {
    $env:FLEXDISPLAY_LISTEN = '127.0.0.1'
    & (Join-Path $PSScriptRoot 'start-usb.ps1') -Silent
}

Add-Content -Path $launcherLog -Value "[*] Waiting for host on port $port" -ErrorAction SilentlyContinue
if (-not (Wait-FlexDisplayHostReady -Port $port)) {
    Add-Content -Path $launcherLog -Value "[ERROR] Host did not start listening on port $port" -ErrorAction SilentlyContinue
    exit 1
}

Add-Content -Path $launcherLog -Value "[*] Opening desktop control panel" -ErrorAction SilentlyContinue
$guiProc = Open-FlexDisplayGui -Root $root -HostIp '127.0.0.1' -Port $port
if (-not $guiProc) {
    Add-Content -Path $launcherLog -Value "[WARN] Could not open Edge/Chrome app window; browse http://127.0.0.1:$port manually" -ErrorAction SilentlyContinue
    exit 0
}

Add-Content -Path $launcherLog -Value "[OK] Desktop control panel launched" -ErrorAction SilentlyContinue
exit 0
