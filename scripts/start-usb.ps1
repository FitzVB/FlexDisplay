# FlexDisplay - USB One-Click Start
# This flow is intended for non-technical usage:
# - Detect/select device
# - Install APK only when missing (or force reinstall)
# - Configure adb reverse
# - Launch Android app
# - Start host server

param(
    [switch]$ForceInstallApk = $false
)

Write-Host ""
Write-Host "FlexDisplay - USB Mode Startup" -ForegroundColor Cyan
Write-Host "===================================" -ForegroundColor Cyan
Write-Host ""

$runtimeEnv = Join-Path $PSScriptRoot "runtime-env.ps1"
if (Test-Path $runtimeEnv) {
    . $runtimeEnv -RootPath (Split-Path -Parent $PSScriptRoot)
}

function Add-PathIfExists {
    param([string]$PathToAdd)
    if ((Test-Path $PathToAdd) -and ($env:Path -notlike "*$PathToAdd*")) {
        $env:Path = $env:Path + ";" + $PathToAdd
    }
}

function Resolve-AdbPath {
    param([string]$Root)

    $candidates = @(
        (Join-Path $env:LOCALAPPDATA "Android\Sdk\platform-tools\adb.exe"),
        (Join-Path $env:USERPROFILE "AppData\Local\Android\Sdk\platform-tools\adb.exe"),
        (Join-Path $Root "adb.exe")
    )

    if ($env:ANDROID_SDK_ROOT) {
        $candidates += (Join-Path $env:ANDROID_SDK_ROOT "platform-tools\adb.exe")
    }
    if ($env:ANDROID_HOME) {
        $candidates += (Join-Path $env:ANDROID_HOME "platform-tools\adb.exe")
    }

    foreach ($candidate in $candidates) {
        if ($candidate -and (Test-Path $candidate)) {
            return $candidate
        }
    }

    $adbCmd = Get-Command adb -ErrorAction SilentlyContinue
    if ($adbCmd) {
        return $adbCmd.Source
    }

    return $null
}

function Parse-ConnectedDevices {
    param([string[]]$AdbDevicesLines)

    $devices = @()
    foreach ($line in ($AdbDevicesLines | Select-Object -Skip 1)) {
        if (-not $line) { continue }
        $trimmed = $line.Trim()
        if ($trimmed -eq "") { continue }
        if ($trimmed -like "List of devices*") { continue }
        if ($trimmed -match "\sdevice($|\s)") {
            $parts = $trimmed -split "\s+"
            if ($parts.Count -gt 0) {
                $serial = $parts[0]
                $details = if ($parts.Count -gt 1) { ($parts | Select-Object -Skip 1) -join " " } else { "" }
                $devices += [PSCustomObject]@{
                    Serial  = $serial
                    Details = $details
                }
            }
        }
    }
    # Force array return so a single device is not unwrapped to a scalar object.
    return ,$devices
}

function Resolve-HostExePath {
    param([string]$Root)

    $candidates = @(
        (Join-Path $Root "host-windows\target\release\host-windows.exe"),
        (Join-Path $Root "host-windows.exe")
    )

    foreach ($candidate in $candidates) {
        if (Test-Path $candidate) {
            return $candidate
        }
    }

    return $null
}

function Resolve-ApkPath {
    param([string]$Root)

    $candidates = @(
        (Join-Path $Root "android-client\app\build\outputs\apk\debug\app-debug.apk"),
        (Join-Path $Root "FlexDisplay.apk")
    )

    foreach ($candidate in $candidates) {
        if (Test-Path $candidate) {
            return $candidate
        }
    }

    return $null
}

function Test-AppInstalled {
    param(
        [string]$AdbPath,
        [string]$Serial,
        [string]$PackageName
    )

    $pmOut = & $AdbPath -s $Serial shell pm path $PackageName 2>&1
    if ($LASTEXITCODE -ne 0) {
        return $false
    }

    foreach ($line in $pmOut) {
        if ($line -match "^package:") {
            return $true
        }
    }

    return $false
}

$root = Split-Path -Parent $PSScriptRoot
Add-PathIfExists (Join-Path $env:LOCALAPPDATA "Android\Sdk\platform-tools")
Add-PathIfExists (Join-Path $env:USERPROFILE "AppData\Local\Android\Sdk\platform-tools")

$adbPath = Resolve-AdbPath -Root $root
if (-not $adbPath) {
    Write-Host "[ERROR] ADB was not found." -ForegroundColor Red
    Write-Host "        Install Android platform-tools or run scripts\setup.ps1 -Full" -ForegroundColor Yellow
    exit 1
}

Write-Host "[*] Using ADB: $adbPath" -ForegroundColor Gray

# Kill any stale ADB daemon before starting — prevents zombie server blocking reconnection
Write-Host '[*] Resetting ADB server (clearing stale connections)...' -ForegroundColor Cyan
& $adbPath kill-server 2>$null | Out-Null

# Register cleanup: runs when terminal window closes or PowerShell engine exits
Register-EngineEvent PowerShell.Exiting -MessageData $adbPath -Action {
    Stop-Process -Name 'host-windows' -Force -ErrorAction SilentlyContinue
    Get-CimInstance Win32_Process -Filter "Name='msedge.exe'" -ErrorAction SilentlyContinue |
        Where-Object { $_.CommandLine -like '*--app=*9001*' } |
        ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
    if ($event.MessageData) { & $event.MessageData kill-server 2>$null }
} | Out-Null

# Detect connected devices
Write-Host "[*] Checking USB devices..." -ForegroundColor Cyan
$adbOut = & $adbPath devices -l
$devices = @(Parse-ConnectedDevices -AdbDevicesLines $adbOut)

$selectedSerial = $null
if ($devices.Count -eq 0) {
    Write-Host "[WARN] No USB devices found. Host will still start." -ForegroundColor Yellow
    Write-Host "       Connect a device and run START.bat again for auto-setup." -ForegroundColor Yellow
} elseif ($devices.Count -eq 1) {
    $selectedSerial = $devices[0].Serial
    Write-Host "[OK] Found 1 device: $selectedSerial" -ForegroundColor Green
} else {
    Write-Host "[OK] Found $($devices.Count) devices:" -ForegroundColor Green
    for ($i = 0; $i -lt $devices.Count; $i++) {
        $d = $devices[$i]
        Write-Host ("  " + ($i + 1) + ") " + $d.Serial + " " + $d.Details) -ForegroundColor Gray
    }

    $pick = Read-Host "Choose device [1-$($devices.Count)]"
    $idx = 0
    if (-not [int]::TryParse($pick, [ref]$idx) -or $idx -lt 1 -or $idx -gt $devices.Count) {
        Write-Host "[WARN] Invalid selection. Using first device." -ForegroundColor Yellow
        $idx = 1
    }
    $selectedSerial = $devices[$idx - 1].Serial
    Write-Host "[OK] Selected: $selectedSerial" -ForegroundColor Green
}

if ($selectedSerial) {
    $packageName = "com.example.tabletmonitor"
    $forceInstall = $ForceInstallApk -or ($env:FLEXDISPLAY_FORCE_APK_INSTALL -eq "1")
    $appInstalled = Test-AppInstalled -AdbPath $adbPath -Serial $selectedSerial -PackageName $packageName

    $apkPath = Resolve-ApkPath -Root $root
    if ($apkPath) {
        if ($appInstalled -and (-not $forceInstall)) {
            Write-Host "[OK] App already installed. Skipping APK install." -ForegroundColor Green
            Write-Host "     To force reinstall: run scripts\start-usb.ps1 -ForceInstallApk" -ForegroundColor Gray
        } else {
            Write-Host "[*] Installing app on device..." -ForegroundColor Cyan
            $installOutput = & $adbPath -s $selectedSerial install -r $apkPath 2>&1
            if ($LASTEXITCODE -eq 0) {
                Write-Host "[OK] App installed/updated" -ForegroundColor Green
            } else {
                Write-Host "[WARN] APK install failed. Continuing anyway." -ForegroundColor Yellow
                $installOutput | ForEach-Object { Write-Host "       $_" -ForegroundColor DarkYellow }
            }
        }
    } else {
        if ($appInstalled) {
            Write-Host "[OK] App already installed on device." -ForegroundColor Green
            Write-Host "     APK file not found locally, skipping install." -ForegroundColor Gray
        } else {
            Write-Host "[WARN] APK not found." -ForegroundColor Yellow
            Write-Host "       Expected one of:" -ForegroundColor Yellow
            Write-Host "       - android-client\app\build\outputs\apk\debug\app-debug.apk" -ForegroundColor DarkYellow
            Write-Host "       - FlexDisplay.apk" -ForegroundColor DarkYellow
            Write-Host "       App is not installed on device, so connection may fail." -ForegroundColor Yellow
        }
    }

    Write-Host "[*] Setting up USB tunnel (adb reverse)..." -ForegroundColor Cyan
    & $adbPath -s $selectedSerial reverse --remove-all | Out-Null
    & $adbPath -s $selectedSerial reverse tcp:9001 tcp:9001 | Out-Null
    if ($LASTEXITCODE -eq 0) {
        Write-Host "[OK] Reverse tcp:9001 ready" -ForegroundColor Green
        Write-Host "     Device will use 127.0.0.1:9001" -ForegroundColor Gray
    } else {
        Write-Host "[WARN] Could not configure adb reverse automatically." -ForegroundColor Yellow
    }

    Write-Host "[*] Launching Android app..." -ForegroundColor Cyan
    & $adbPath -s $selectedSerial shell monkey -p $packageName -c android.intent.category.LAUNCHER 1 | Out-Null
}

Write-Host ""
Write-Host "[*] Starting server..." -ForegroundColor Cyan

Stop-Process -Name "host-windows" -Force -ErrorAction SilentlyContinue

$portOwners = Get-NetTCPConnection -LocalPort 9001 -State Listen -ErrorAction SilentlyContinue |
    Select-Object -ExpandProperty OwningProcess -Unique
foreach ($ownerPid in $portOwners) {
    if ($ownerPid -and $ownerPid -ne $PID) {
        Stop-Process -Id $ownerPid -Force -ErrorAction SilentlyContinue
    }
}

$env:TABLET_MONITOR_LISTEN = '127.0.0.1'
$env:TABLET_MONITOR_FPS = '60'

function Invoke-Cleanup {
    param([string]$AdbExe)
    Stop-Process -Name 'host-windows' -Force -ErrorAction SilentlyContinue
    Get-CimInstance Win32_Process -Filter "Name='msedge.exe'" -ErrorAction SilentlyContinue |
        Where-Object { $_.CommandLine -like '*--app=*9001*' } |
        ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
    if ($AdbExe -and (Test-Path $AdbExe)) { & $AdbExe kill-server 2>$null | Out-Null }
    Write-Host '[OK] Cleanup done.' -ForegroundColor Green
}

$hostExe = Resolve-HostExePath -Root $root
if ($hostExe) {
    $hostDir = Split-Path -Parent $hostExe
    Set-Location $hostDir
    try {
        & $hostExe
        $exitCode = $LASTEXITCODE
    } finally {
        Invoke-Cleanup -AdbExe $adbPath
    }
    exit $exitCode
}

if (Test-Path (Join-Path $root "host-windows\Cargo.toml")) {
    Set-Location (Join-Path $root "host-windows")
    try {
        cargo run --release
        $exitCode = $LASTEXITCODE
    } finally {
        Invoke-Cleanup -AdbExe $adbPath
    }
    exit $exitCode
}

Write-Host "[ERROR] Host executable not found." -ForegroundColor Red
Write-Host "        Expected one of:" -ForegroundColor Red
Write-Host "        - host-windows\target\release\host-windows.exe" -ForegroundColor DarkRed
Write-Host "        - host-windows.exe" -ForegroundColor DarkRed
exit 1
