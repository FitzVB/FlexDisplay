#Requires -Version 5.1
param(
    [string]$RootPath,
    [switch]$EnsureAdb = $true,
    [switch]$EnsureFfmpeg = $true
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

if (-not $RootPath) {
    $RootPath = Split-Path -Parent $PSScriptRoot
}

$runtimeRoot = Join-Path $RootPath ".runtime"
$cacheDir = Join-Path $RootPath ".cache"

function Write-Step([string]$Message) {
    Write-Host "[*] $Message" -ForegroundColor Cyan
}

function Write-Ok([string]$Message) {
    Write-Host "[OK] $Message" -ForegroundColor Green
}

function Invoke-Download([string]$Url, [string]$Dest) {
    if (Test-Path $Dest) {
        return
    }
    $tmp = "$Dest.tmp"
    Invoke-WebRequest -Uri $Url -OutFile $tmp -UseBasicParsing
    Move-Item $tmp $Dest -Force
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

if (-not (Test-Path $runtimeRoot)) {
    New-Item -ItemType Directory -Path $runtimeRoot -Force | Out-Null
}
if (-not (Test-Path $cacheDir)) {
    New-Item -ItemType Directory -Path $cacheDir -Force | Out-Null
}

if ($EnsureAdb) {
    $adbExe = Join-Path $runtimeRoot "adb\platform-tools\adb.exe"
    if (-not (Test-Path $adbExe)) {
        Write-Step "Preparing local ADB runtime"
        if (Get-Command Update-FlexDisplayStartupProgress -ErrorAction SilentlyContinue) {
            Update-FlexDisplayStartupProgress -Percent 10 -Message 'Descargando herramientas Android (ADB)...' -Marquee
        }
        $adbZip = Join-Path $cacheDir "platform-tools-windows.zip"
        Invoke-Download -Url "https://dl.google.com/android/repository/platform-tools-latest-windows.zip" -Dest $adbZip

        $adbRoot = Join-Path $runtimeRoot "adb\platform-tools"
        $adbMap = @{
            "adb.exe" = (Join-Path $adbRoot "adb.exe")
            "AdbWinApi.dll" = (Join-Path $adbRoot "AdbWinApi.dll")
            "AdbWinUsbApi.dll" = (Join-Path $adbRoot "AdbWinUsbApi.dll")
        }
        Extract-FromZipByLeaf -ZipPath $adbZip -LeafToDestination $adbMap
    }

    if (-not (Test-Path $adbExe)) {
        throw "Could not prepare local ADB runtime"
    }
    Write-Ok "ADB runtime ready"
    if (Get-Command Update-FlexDisplayStartupProgress -ErrorAction SilentlyContinue) {
        Update-FlexDisplayStartupProgress -Percent 18 -Message 'ADB listo'
    }
}

if ($EnsureFfmpeg) {
    $ffmpegExe = Join-Path $runtimeRoot "ffmpeg\bin\ffmpeg.exe"
    if (-not (Test-Path $ffmpegExe)) {
        Write-Step "Preparing local FFmpeg runtime"
        if (Get-Command Update-FlexDisplayStartupProgress -ErrorAction SilentlyContinue) {
            Update-FlexDisplayStartupProgress -Percent 20 -Message 'Descargando FFmpeg...' -Marquee
        }
        $ffZip = Join-Path $cacheDir "ffmpeg-win64-gyan-release.zip"
        # Prefer stable release builds over bleeding-edge master to improve NVENC
        # compatibility on machines with slightly older NVIDIA drivers.
        Invoke-Download -Url "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip" -Dest $ffZip

        Add-Type -AssemblyName System.IO.Compression.FileSystem
        $zip = [System.IO.Compression.ZipFile]::OpenRead($ffZip)
        try {
            $ffRoot = Join-Path $runtimeRoot "ffmpeg\bin"
            $ffDest = Join-Path $ffRoot "ffmpeg.exe"
            foreach ($entry in $zip.Entries) {
                if ($entry.FullName -match "/bin/ffmpeg\.exe$") {
                    if (-not (Test-Path $ffRoot)) {
                        New-Item -ItemType Directory -Path $ffRoot -Force | Out-Null
                    }
                    [System.IO.Compression.ZipFileExtensions]::ExtractToFile($entry, $ffDest, $true)
                    break
                }
            }
        } finally {
            $zip.Dispose()
        }
    }

    if (-not (Test-Path $ffmpegExe)) {
        throw "Could not prepare local FFmpeg runtime"
    }
    Write-Ok "FFmpeg runtime ready"
    if (Get-Command Update-FlexDisplayStartupProgress -ErrorAction SilentlyContinue) {
        Update-FlexDisplayStartupProgress -Percent 25 -Message 'FFmpeg listo'
    }
}
