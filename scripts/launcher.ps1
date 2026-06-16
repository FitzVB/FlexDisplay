# FlexDisplay - Simple Launcher
# No ADB automation or APK install

Write-Host ""
Write-Host "FlexDisplay - Launcher" -ForegroundColor Cyan
Write-Host "=========================" -ForegroundColor Cyan
Write-Host ""

$root = Split-Path -Parent $PSScriptRoot
$usbScript = Join-Path $PSScriptRoot "start-usb.ps1"
$wifiScript = Join-Path $PSScriptRoot "start-wifi.ps1"
$ensureRuntimeScript = Join-Path $PSScriptRoot "ensure-runtime.ps1"

function Test-VirtualDisplayDriverInstalled {
    # Match any indirect display adapter installed as ROOT\DISPLAY (all VDD variants)
    # or by known friendly name patterns used across different VDD package versions.
    $driverPresent = Get-PnpDevice -Class Display -ErrorAction SilentlyContinue |
    Where-Object {
        $_.InstanceId -like 'ROOT\DISPLAY\*' -or
        $_.FriendlyName -like '*Virtual*Display*' -or
        $_.FriendlyName -like '*Virtual*Monitor*' -or
        $_.FriendlyName -like '*VDD*'
    }
    if ($driverPresent) { return $true }

    $monitorPresent = Get-PnpDevice -Class Monitor -ErrorAction SilentlyContinue |
    Where-Object { $_.FriendlyName -like '*VDD*' -or $_.FriendlyName -like '*Virtual*' }
    return ($null -ne $monitorPresent)
}

# VDD check — informational only, no auto-install
Write-Host '[*] Checking virtual display driver...' -ForegroundColor Cyan
if (Test-VirtualDisplayDriverInstalled) {
    $errDevice = Get-PnpDevice -Class Display -ErrorAction SilentlyContinue |
    Where-Object {
        ($_.InstanceId -like 'ROOT\DISPLAY\*' -or
        $_.FriendlyName -like '*Virtual*Display*' -or
        $_.FriendlyName -like '*Virtual*Monitor*' -or
        $_.FriendlyName -like '*VDD*') -and
        $_.Status -eq 'Error'
    }
    if ($errDevice) {
        Write-Host '[WARN] Virtual display device is in error state.' -ForegroundColor Yellow
        Write-Host '       Go to Device Manager -> Display adapters -> right-click -> Enable.' -ForegroundColor DarkYellow
    }
    else {
        Write-Host '[OK] Virtual display driver detected' -ForegroundColor Green
    }
}
else {
    Write-Host '[WARN] Virtual display driver not found.' -ForegroundColor Yellow
    Write-Host '       Extended mode will not be available.' -ForegroundColor Yellow
    Write-Host '       Extended mode: install Virtual Display Driver from GitHub (see QUICK-START.md).' -ForegroundColor DarkYellow
}

if (Test-Path $ensureRuntimeScript) {
    Write-Host "[*] Preparing runtime (ADB + FFmpeg)..." -ForegroundColor Cyan
    Write-Host "    First run downloads tools from official sources (~110 MB, one time)." -ForegroundColor DarkGray
    try {
        & $ensureRuntimeScript -RootPath $root -EnsureAdb -EnsureFfmpeg
        Write-Host "[OK] Runtime ready" -ForegroundColor Green
    }
    catch {
        Write-Host "[WARN] Runtime download failed: $($_.Exception.Message)" -ForegroundColor Yellow
        Write-Host "       Check your internet connection and run START.bat again." -ForegroundColor Yellow
        Write-Host "       USB/Wi-Fi mode needs ADB and FFmpeg in .runtime\" -ForegroundColor Yellow
        $cont = Read-Host "Continue anyway? [y/N]"
        if ($cont -notmatch '^[yY]') { exit 1 }
    }
}

# Ask for connection mode
Write-Host "Select connection mode:" -ForegroundColor Gray
Write-Host "  1) USB (recommended)" -ForegroundColor Gray
Write-Host "  2) Wi-Fi" -ForegroundColor Gray
$pick = Read-Host "Mode [1/2]"

$mode = if ($pick -eq "2") { "wifi" } else { "usb" }

if ($mode -eq "usb") {
    Write-Host ""
    Write-Host "[INFO] USB mode selected" -ForegroundColor Cyan
    if (-not (Test-Path $usbScript)) {
        Write-Host "[ERROR] scripts\start-usb.ps1 not found" -ForegroundColor Red
        exit 1
    }
    & $usbScript
}
else {
    Write-Host ""
    Write-Host "[INFO] Wi-Fi mode selected" -ForegroundColor Cyan
    if (-not (Test-Path $wifiScript)) {
        Write-Host "[ERROR] scripts\start-wifi.ps1 not found" -ForegroundColor Red
        exit 1
    }
    & $wifiScript
}
