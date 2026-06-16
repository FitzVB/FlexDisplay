# health-check.ps1 - System health check for FlexDisplay
# Verifies all dependencies and configurations are in order
# Usage: .\health-check.ps1 [-Full] [-Fix]

param(
    [switch]$Full = $false,
    [switch]$Fix = $false
)

$ErrorActionPreference = "SilentlyContinue"
$ScriptPath = Split-Path -Parent $MyInvocation.MyCommand.Path
$HealthStatus = @{ OK = 0; WARNING = 0; ERROR = 0 }

Write-Host "==============================" -ForegroundColor Cyan
Write-Host "FlexDisplay - Health Check" -ForegroundColor Cyan
Write-Host "==============================`n" -ForegroundColor Cyan

# Helper functions
function Check-Tool {
    param([string]$Name, [string]$Command)

    try {
        $null = & $Command --version 2>&1
        Write-Host "✓ $Name" -ForegroundColor Green
        $HealthStatus.OK++
        return $true
    } catch {
        Write-Host "✗ $Name NOT FOUND" -ForegroundColor Red
        $HealthStatus.ERROR++
        return $false
    }
}

function Check-Path {
    param([string]$Name, [string]$Path)

    if (Test-Path $Path) {
        Write-Host "✓ $Name" -ForegroundColor Green
        $HealthStatus.OK++
        return $true
    } else {
        Write-Host "✗ $Name NOT FOUND: $Path" -ForegroundColor Red
        $HealthStatus.ERROR++
        return $false
    }
}

function Check-Port {
    param([int]$Port, [string]$Description)

    try {
        $tcpClient = New-Object System.Net.Sockets.TcpClient
        $tcpClient.Connect("127.0.0.1", $Port)
        $tcpClient.Close()
        Write-Host "⚠ Port $Port in use ($Description) - server may already be running" -ForegroundColor Yellow
        $HealthStatus.WARNING++
    } catch {
        Write-Host "✓ Port $Port available ($Description)" -ForegroundColor Green
        $HealthStatus.OK++
    }
}

function Check-ADBDevices {
    $devices = adb devices 2>&1 | Select-Object -Skip 1 | Where-Object { $_ -match " device" }

    if ($devices) {
        Write-Host "✓ Android devices connected: $($devices.Count)" -ForegroundColor Green
        $HealthStatus.OK++
        return $true
    } else {
        Write-Host "⚠ No Android devices found (is USB cable connected?)" -ForegroundColor Yellow
        $HealthStatus.WARNING++
        return $false
    }
}

function Check-ABDReverse {
    try {
        $result = adb reverse -l 2>&1
        if ($result -match "9001") {
            Write-Host "✓ ADB reverse tunnel established (tcp:9001)" -ForegroundColor Green
            $HealthStatus.OK++
            return $true
        } else {
            Write-Host "⚠ ADB reverse tunnel not active" -ForegroundColor Yellow
            $HealthStatus.WARNING++
            return $false
        }
    } catch {
        Write-Host "⚠ Could not check ADB reverse" -ForegroundColor Yellow
        $HealthStatus.WARNING++
        return $false
    }
}

function Check-FFmpegEncoders {
    $lib = Join-Path $ScriptPath "lib\Common.ps1"
    if (Test-Path $lib) {
        . $lib
        $available = Get-VendorFilteredEncoders
    } else {
        $encoders = @("h264_nvenc", "h264_qsv", "h264_amf", "libx264")
        $available = @()
        $text = ffmpeg -hide_banner -encoders 2>&1 | Out-String
        foreach ($encoder in $encoders) {
            if ($text -match $encoder) { $available += $encoder }
        }
    }

    if ($available.Count -gt 0) {
        Write-Host "✓ Available H.264 encoders (vendor-filtered): $($available -join ', ')" -ForegroundColor Green
        $HealthStatus.OK++
        return $true
    } else {
        Write-Host "✗ No supported H.264 encoders found" -ForegroundColor Red
        $HealthStatus.ERROR++
        return $false
    }
}

function Fix-Issues {
    Write-Host "`nAttempting to fix issues..." -ForegroundColor Yellow

    # Reestablish ADB reverse if needed
    $reverse = adb reverse -l 2>&1 | Select-String "9001"
    if (-not $reverse) {
        Write-Host "Re-establishing ADB reverse..."
        adb reverse tcp:9001 tcp:9001
        Write-Host "✓ ADB reverse re-established" -ForegroundColor Green
    }

    # Reconnect devices
    Write-Host "Reconnecting devices..."
    adb disconnect
    Start-Sleep -Seconds 1
    adb devices
}

# Start checks
Write-Host "Basic Tools`n" -ForegroundColor Cyan

Check-Tool "Rust (rustc)" rustc
Check-Tool "Cargo" cargo
Check-Tool "ADB" adb
Check-Tool "Java" java
Check-Tool "FFmpeg" ffmpeg
Check-Tool "Git" git

# Project structure
Write-Host "`nProject Structure`n" -ForegroundColor Cyan

Check-Path "Rust project" "$ScriptPath\host-windows\Cargo.toml"
Check-Path "Android project" "$ScriptPath\android-client\build.gradle.kts"
Check-Path "Setup script" "$ScriptPath\setup.ps1"
Check-Path "Build script" "$ScriptPath\build.ps1"

# USB Configuration
Write-Host "`nUSB & Device Configuration`n" -ForegroundColor Cyan

Check-Port 9001 "FlexDisplay"
Check-ADBDevices
Check-ABDReverse

# FFmpeg configuration
Write-Host "`nFFmpeg H.264 Encoders`n" -ForegroundColor Cyan

Check-FFmpegEncoders

# Detailed diagnostics (if -Full)
if ($Full) {
    Write-Host "`nDetailed Diagnostics`n" -ForegroundColor Cyan

    Write-Host "Rust version details:"
    rustc --version --verbose

    Write-Host "`nJava version details:"
    java -version

    Write-Host "`nFFmpeg version details:"
    ffmpeg -version | Select-Object -First 5

    Write-Host "`nGit configuration:"
    git config --list | Select-Object -First 10

    Write-Host "`nAndroid devices detailed:"
    adb devices -l
}

# Summary
Write-Host "`n==============================" -ForegroundColor Cyan
Write-Host "Health Check Summary" -ForegroundColor Cyan
Write-Host "==============================" -ForegroundColor Cyan

$total = $HealthStatus.OK + $HealthStatus.WARNING + $HealthStatus.ERROR
Write-Host "✓ OK:      $($HealthStatus.OK)" -ForegroundColor Green
Write-Host "⚠ WARNING: $($HealthStatus.WARNING)" -ForegroundColor Yellow
Write-Host "✗ ERROR:   $($HealthStatus.ERROR)" -ForegroundColor Red
Write-Host "`nTotal checks: $total"

if ($HealthStatus.ERROR -gt 0) {
    Write-Host "`nStatus: UNHEALTHY" -ForegroundColor Red
    Write-Host "Run .\setup.ps1 -Install to fix missing dependencies" -ForegroundColor Yellow
    if ($Fix) { Fix-Issues }
    exit 1
} elseif ($HealthStatus.WARNING -gt 0) {
    Write-Host "`nStatus: DEGRADED" -ForegroundColor Yellow
    if ($Fix) { Fix-Issues }
    exit 0
} else {
    Write-Host "`nStatus: HEALTHY ✓" -ForegroundColor Green
    exit 0
}
