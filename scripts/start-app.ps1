#Requires -Version 5.1
<#!
.SYNOPSIS
Silent desktop launcher - no console window. Opens the host control app in Edge/Chrome app mode.
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

Start-FlexDisplayStartupSplash
Update-FlexDisplayStartupProgress -Percent 3 -Message 'Iniciando FlexDisplay...'

try {
    Start-Transcript -Path $launcherLog -Append -ErrorAction SilentlyContinue | Out-Null

    $ensureRuntimeScript = Join-Path $PSScriptRoot 'ensure-runtime.ps1'
    if (Test-Path $ensureRuntimeScript) {
        try {
            Update-FlexDisplayStartupProgress -Percent 8 -Message 'Preparando componentes (la primera vez puede tardar)...'
            & $ensureRuntimeScript -RootPath $root -EnsureAdb -EnsureFfmpeg
            Update-FlexDisplayStartupProgress -Percent 26 -Message 'Componentes listos'
        }
        catch {
            Add-Content -Path $launcherLog -Value "[WARN] Runtime prepare failed: $($_.Exception.Message)" -ErrorAction SilentlyContinue
            Update-FlexDisplayStartupProgress -Message 'Advertencia: algunos componentes no se prepararon' -Error
            Start-Sleep -Seconds 2
        }
    }

    Stop-Transcript -ErrorAction SilentlyContinue | Out-Null

    $env:FLEXDISPLAY_FPS = '60'
    $port = 9001
    $ErrorActionPreference = 'Continue'

    if ($Mode -eq 'WiFi') {
        Update-FlexDisplayStartupProgress -Percent 30 -Message 'Iniciando modo Wi-Fi...'
        Remove-Item Env:FLEXDISPLAY_LISTEN -ErrorAction SilentlyContinue
        & (Join-Path $PSScriptRoot 'start-wifi.ps1') -Silent
    }
    else {
        Update-FlexDisplayStartupProgress -Percent 30 -Message 'Iniciando modo USB...'
        $env:FLEXDISPLAY_LISTEN = '127.0.0.1'
        & (Join-Path $PSScriptRoot 'start-usb.ps1') -Silent
    }

    Update-FlexDisplayStartupProgress -Percent 82 -Message 'Esperando servidor...'
    Add-Content -Path $launcherLog -Value "[*] Waiting for host on port $port" -ErrorAction SilentlyContinue
    if (-not (Wait-FlexDisplayHostReady -Port $port)) {
        Add-Content -Path $launcherLog -Value "[ERROR] Host did not start listening on port $port" -ErrorAction SilentlyContinue
        Update-FlexDisplayStartupProgress -Percent 0 -Message 'Error: el servidor no arranco. Revisa logs/flexdisplay-start.log' -Error
        Start-Sleep -Seconds 4
        exit 1
    }

    Update-FlexDisplayStartupProgress -Percent 92 -Message 'Abriendo panel de control...'
    Add-Content -Path $launcherLog -Value "[*] Opening desktop control panel" -ErrorAction SilentlyContinue
    $guiProc = Open-FlexDisplayGui -Root $root -HostIp '127.0.0.1' -Port $port
    if (-not $guiProc) {
        Add-Content -Path $launcherLog -Value "[WARN] Could not open Edge/Chrome app window; browse http://127.0.0.1:$port manually" -ErrorAction SilentlyContinue
        Update-FlexDisplayStartupProgress -Message 'Abre http://127.0.0.1:9001 en el navegador' -Error
        Start-Sleep -Seconds 3
        exit 0
    }

    Add-Content -Path $launcherLog -Value "[OK] Desktop control panel launched" -ErrorAction SilentlyContinue
    Complete-FlexDisplayStartupSplash
    exit 0
}
catch {
    Add-Content -Path $launcherLog -Value "[ERROR] Launcher failed: $($_.Exception.Message)" -ErrorAction SilentlyContinue
    Update-FlexDisplayStartupProgress -Message "Error: $($_.Exception.Message)" -Error
    Start-Sleep -Seconds 4
    exit 1
}
finally {
    Stop-FlexDisplayStartupSplash
}
