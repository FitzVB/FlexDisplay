#Requires -Version 5.1
<#!
.SYNOPSIS
Create a lightweight precompiled distribution package.

.DESCRIPTION
- Builds host executable in release mode.
- Optionally builds Android debug APK (or reuses existing one).
- Bundles minimal runtime dependencies (ADB + FFmpeg).
- Copies startup scripts and creates ZIP under dist/.

.PARAMETER Version
Optional package version override. Defaults to host-windows/Cargo.toml version.

.PARAMETER BuildAndroid
Build Android APK before packaging.

.PARAMETER SkipAndroid
Do not include Android APK.

.PARAMETER CacheDir
Download cache directory. Default: .cache

.PARAMETER OutputDir
Distribution output directory. Default: dist

.PARAMETER SkipBundledRuntime
Do not include ADB/FFmpeg binaries in package. Runtime is downloaded on first run.
#>
param(
    [string]$Version = "",
    [switch]$BuildAndroid,
    [switch]$SkipAndroid,
    [switch]$SkipBundledRuntime,
    [string]$CacheDir = "",
    [string]$OutputDir = ""
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$RepoRoot = Split-Path -Parent $PSScriptRoot
if (-not $CacheDir) {
    $CacheDir = Join-Path $RepoRoot ".cache"
}
if (-not $OutputDir) {
    $OutputDir = Join-Path $RepoRoot "dist"
}

function Write-Step([string]$Message) {
    Write-Host "`n==> $Message" -ForegroundColor Cyan
}

function Write-Ok([string]$Message) {
    Write-Host "    [OK] $Message" -ForegroundColor Green
}

function Assert-Command([string]$Command) {
    if (-not (Get-Command $Command -ErrorAction SilentlyContinue)) {
        throw "Missing command: $Command"
    }
}

function Invoke-Download([string]$Url, [string]$Dest) {
    if (Test-Path $Dest) {
        Write-Ok "Cache hit: $(Split-Path $Dest -Leaf)"
        return
    }

    Write-Host "    Downloading $(Split-Path $Dest -Leaf)"
    $tmp = "$Dest.tmp"
    Invoke-WebRequest -Uri $Url -OutFile $tmp -UseBasicParsing
    Move-Item $tmp $Dest -Force
}

function Get-VersionFromCargo {
    $cargoToml = Join-Path $RepoRoot "host-windows\Cargo.toml"
    if (-not (Test-Path $cargoToml)) {
        return "0.0.0"
    }

    $line = Select-String -Path $cargoToml -Pattern 'version\s*=\s*"([^"]+)"' | Select-Object -First 1
    if ($line) {
        return $line.Matches[0].Groups[1].Value
    }

    return "0.0.0"
}

function Build-HostRelease {
    Write-Step "Building host (release)"
    Assert-Command "cargo"

    Push-Location (Join-Path $RepoRoot "host-windows")
    try {
        cargo build --release
        if ($LASTEXITCODE -ne 0) {
            throw "cargo build --release failed"
        }
    } finally {
        Pop-Location
    }

    $hostExe = Join-Path $RepoRoot "host-windows\target\release\host-windows.exe"
    if (-not (Test-Path $hostExe)) {
        throw "host-windows.exe not found after build"
    }

    Write-Ok "Host binary ready"
    return $hostExe
}

function Resolve-ApkPath {
    $apk = Join-Path $RepoRoot "android-client\app\build\outputs\apk\debug\app-debug.apk"

    if ($BuildAndroid) {
        Write-Step "Building Android APK (debug)"
        Push-Location (Join-Path $RepoRoot "android-client")
        try {
            .\gradlew.bat assembleDebug
            if ($LASTEXITCODE -ne 0) {
                throw "Android build failed"
            }
        } finally {
            Pop-Location
        }
    }

    if (Test-Path $apk) {
        Write-Ok "APK ready"
        return $apk
    }

    if ($SkipAndroid) {
        Write-Ok "Skipping APK (SkipAndroid)"
        return $null
    }

    Write-Host "    [WARN] APK not found; package will be created without APK" -ForegroundColor Yellow
    return $null
}

function Extract-FromZipByLeaf {
    param(
        [string]$ZipPath,
        [hashtable]$LeafToDestination
    )

    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $zip = [System.IO.Compression.ZipFile]::OpenRead($ZipPath)
    try {
        foreach ($entry in $zip.Entries) {
            $leaf = [System.IO.Path]::GetFileName($entry.FullName)
            if (-not $leaf) { continue }
            if ($LeafToDestination.ContainsKey($leaf)) {
                $dest = $LeafToDestination[$leaf]
                $destDir = Split-Path -Parent $dest
                if (-not (Test-Path $destDir)) {
                    New-Item -ItemType Directory -Path $destDir -Force | Out-Null
                }
                [System.IO.Compression.ZipFileExtensions]::ExtractToFile($entry, $dest, $true)
            }
        }
    } finally {
        $zip.Dispose()
    }
}

function Prepare-MinRuntime {
    param([string]$DistRoot)

    Write-Step "Preparing minimal runtime (ADB + FFmpeg)"
    New-Item -ItemType Directory -Force -Path $CacheDir | Out-Null

    $adbZip = Join-Path $CacheDir "platform-tools-windows.zip"
    $ffZip = Join-Path $CacheDir "ffmpeg-win64-gyan-release.zip"

    Invoke-Download -Url "https://dl.google.com/android/repository/platform-tools-latest-windows.zip" -Dest $adbZip
    # Prefer stable release builds over bleeding-edge master to improve NVENC
    # compatibility on machines with slightly older NVIDIA drivers.
    Invoke-Download -Url "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip" -Dest $ffZip

    $adbRoot = Join-Path $DistRoot ".runtime\adb\platform-tools"
    $ffRoot = Join-Path $DistRoot ".runtime\ffmpeg\bin"

    $adbMap = @{
        "adb.exe" = (Join-Path $adbRoot "adb.exe")
        "AdbWinApi.dll" = (Join-Path $adbRoot "AdbWinApi.dll")
        "AdbWinUsbApi.dll" = (Join-Path $adbRoot "AdbWinUsbApi.dll")
    }

    Extract-FromZipByLeaf -ZipPath $adbZip -LeafToDestination $adbMap

    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $zip = [System.IO.Compression.ZipFile]::OpenRead($ffZip)
    try {
        $ffmpegDest = Join-Path $ffRoot "ffmpeg.exe"
        foreach ($entry in $zip.Entries) {
            if ($entry.FullName -match "/bin/ffmpeg\.exe$") {
                $ffDir = Split-Path -Parent $ffmpegDest
                if (-not (Test-Path $ffDir)) {
                    New-Item -ItemType Directory -Path $ffDir -Force | Out-Null
                }
                [System.IO.Compression.ZipFileExtensions]::ExtractToFile($entry, $ffmpegDest, $true)
                break
            }
        }
    } finally {
        $zip.Dispose()
    }

    if (-not (Test-Path (Join-Path $adbRoot "adb.exe"))) {
        throw "Failed to prepare runtime ADB"
    }
    if (-not (Test-Path (Join-Path $ffRoot "ffmpeg.exe"))) {
        throw "Failed to prepare runtime FFmpeg"
    }

    Write-Ok "Runtime files ready"
}

function Copy-RequiredFiles {
    param(
        [string]$DistRoot,
        [string]$HostExe,
        [string]$ApkPath
    )

    Write-Step "Copying package files"
    New-Item -ItemType Directory -Force -Path (Join-Path $DistRoot "scripts") | Out-Null

    Copy-Item $HostExe (Join-Path $DistRoot "host-windows.exe") -Force

    if ($ApkPath -and (Test-Path $ApkPath)) {
        Copy-Item $ApkPath (Join-Path $DistRoot "FlexDisplay.apk") -Force
    }

    Copy-Item (Join-Path $RepoRoot "START.bat") (Join-Path $DistRoot "START.bat") -Force
    foreach ($safeBat in @("START_SAFE.bat", "USB_SAFE.bat", "WIFI_SAFE.bat")) {
        $safeSrc = Join-Path $RepoRoot $safeBat
        if (Test-Path $safeSrc) {
            Copy-Item $safeSrc (Join-Path $DistRoot $safeBat) -Force
        }
    }

    $scriptFiles = @(
        "launcher.ps1",
        "ensure-runtime.ps1",
        "runtime-env.ps1",
        "start-usb.ps1",
        "start-wifi.ps1",
        "stop-usb.ps1",
        "install-virtual-display.ps1",
        "remove-virtual-display.ps1",
        "STOP.bat"
    )

    foreach ($file in $scriptFiles) {
        $src = Join-Path $RepoRoot "scripts\$file"
        if (Test-Path $src) {
            Copy-Item $src (Join-Path $DistRoot "scripts\$file") -Force
        }
    }

    if (Test-Path (Join-Path $RepoRoot "README.md")) {
        Copy-Item (Join-Path $RepoRoot "README.md") (Join-Path $DistRoot "README.md") -Force
    }
    foreach ($guide in @("QUICK-START.md", "QUICK-START.es.md")) {
        $src = Join-Path $RepoRoot $guide
        if (Test-Path $src) {
            Copy-Item $src (Join-Path $DistRoot $guide) -Force
        }
    }

    $quick = @"
FlexDisplay - Quick Start

1. Double-click START.bat
2. Select USB or Wi-Fi mode
3. In USB mode the app is auto-installed when an Android device is connected

First run:
  ADB and FFmpeg are downloaded automatically from official sources on first launch.
  An internet connection is required the first time only.

Extended display (optional):
  If you want to use your Android device as an extra monitor (not just a mirror),
  you need to install the Virtual Display Driver first:
  https://github.com/VirtualDrivers/Virtual-Display-Driver/releases
  Download the .exe installer, run it, click Install, then reboot once.

For the full manual: see README.md or https://github.com/FitzVB/tablet-second-monitor
"@
    Set-Content -Path (Join-Path $DistRoot "QUICK_START.txt") -Value $quick -Encoding UTF8

    Write-Ok "Files copied"
}

function Measure-DirMB([string]$Path) {
    if (-not (Test-Path $Path)) { return 0 }
    $sum = (Get-ChildItem -LiteralPath $Path -Recurse -File | Measure-Object Length -Sum).Sum
    return [math]::Round($sum / 1MB, 1)
}

if (-not $Version) {
    $Version = Get-VersionFromCargo
}

$distName = "FlexDisplay-v$Version-windows-lite"
$distRoot = Join-Path $OutputDir $distName
$zipPath = Join-Path $OutputDir "$distName.zip"

Write-Step "Packaging lightweight precompiled distribution"
Write-Host "    Version: $Version"
Write-Host "    Dist dir: $distRoot"

if (Test-Path $distRoot) {
    Remove-Item $distRoot -Recurse -Force
}
if (Test-Path $zipPath) {
    Remove-Item $zipPath -Force
}

$hostExe = Build-HostRelease
$apkPath = Resolve-ApkPath
if (-not $SkipBundledRuntime) {
    Write-Step "Skipping bundled runtime by default (will download on first run)"
    Write-Host "    Pass -SkipBundledRuntime:$false to bundle ADB + FFmpeg inside the ZIP." -ForegroundColor Gray
} else {
    Prepare-MinRuntime -DistRoot $distRoot
}
Copy-RequiredFiles -DistRoot $distRoot -HostExe $hostExe -ApkPath $apkPath

Write-Step "Creating ZIP"
Add-Type -AssemblyName System.IO.Compression.FileSystem
[System.IO.Compression.ZipFile]::CreateFromDirectory($distRoot, $zipPath)

$folderMB = Measure-DirMB $distRoot
$zipMB = [math]::Round((Get-Item $zipPath).Length / 1MB, 1)

Write-Host ""
Write-Host "============================================================" -ForegroundColor Green
Write-Host "PACKAGE READY" -ForegroundColor Green
Write-Host "Dist folder: $distRoot ($folderMB MB)" -ForegroundColor Green
Write-Host "ZIP file:    $zipPath ($zipMB MB)" -ForegroundColor Green
Write-Host "============================================================" -ForegroundColor Green
