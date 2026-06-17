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

function Test-SystemAdbAvailable {
    $candidates = @(
        (Join-Path $env:LOCALAPPDATA 'Android\Sdk\platform-tools\adb.exe'),
        (Join-Path $env:USERPROFILE 'AppData\Local\Android\Sdk\platform-tools\adb.exe')
    )
    foreach ($candidate in $candidates) {
        if (Test-Path -LiteralPath $candidate) {
            return $true
        }
    }
    return [bool](Get-Command adb -ErrorAction SilentlyContinue)
}

function Test-SystemFfmpegAvailable {
    return [bool](Get-Command ffmpeg -ErrorAction SilentlyContinue)
}

function Install-WingetPackage {
    param([string]$PackageId)

    if (-not (Get-Command winget -ErrorAction SilentlyContinue)) {
        return $false
    }

    & winget install --id $PackageId -e --accept-package-agreements --accept-source-agreements --scope user --silent 2>&1 | Out-Null
    return ($LASTEXITCODE -eq 0) -or ($LASTEXITCODE -eq -1979565189)
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
    if (-not (Test-Path $adbExe) -and -not (Test-SystemAdbAvailable)) {
        Write-Step "Preparing ADB runtime"
        if (Get-Command Update-FlexDisplayStartupProgress -ErrorAction SilentlyContinue) {
            Update-FlexDisplayStartupProgress -Percent 10 -Message 'Instalando herramientas Android (ADB)...' -Marquee
        }

        $wingetOk = Install-WingetPackage -PackageId 'Google.PlatformTools'
        if (-not (Test-SystemAdbAvailable)) {
            if (Get-Command Update-FlexDisplayStartupProgress -ErrorAction SilentlyContinue) {
                Update-FlexDisplayStartupProgress -Percent 12 -Message 'Descargando ADB (plan B)...' -Marquee
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
        elseif ($wingetOk) {
            Write-Ok "ADB installed via winget (signed package)"
        }
    }

    if (-not (Test-Path $adbExe) -and -not (Test-SystemAdbAvailable)) {
        throw "Could not prepare local ADB runtime"
    }
    Write-Ok "ADB runtime ready"
    if (Get-Command Update-FlexDisplayStartupProgress -ErrorAction SilentlyContinue) {
        Update-FlexDisplayStartupProgress -Percent 18 -Message 'ADB listo'
    }
}

if ($EnsureFfmpeg) {
    $ffmpegExe = Join-Path $runtimeRoot "ffmpeg\bin\ffmpeg.exe"
    if (-not (Test-Path $ffmpegExe) -and -not (Test-SystemFfmpegAvailable)) {
        Write-Step "Preparing FFmpeg runtime"
        if (Get-Command Update-FlexDisplayStartupProgress -ErrorAction SilentlyContinue) {
            Update-FlexDisplayStartupProgress -Percent 20 -Message 'Instalando FFmpeg...' -Marquee
        }

        $wingetOk = Install-WingetPackage -PackageId 'Gyan.FFmpeg'
        if (-not (Test-SystemFfmpegAvailable)) {
            if (Get-Command Update-FlexDisplayStartupProgress -ErrorAction SilentlyContinue) {
                Update-FlexDisplayStartupProgress -Percent 22 -Message 'Descargando FFmpeg (plan B)...' -Marquee
            }
            $ffZip = Join-Path $cacheDir "ffmpeg-win64-gyan-release.zip"
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
        elseif ($wingetOk) {
            Write-Ok "FFmpeg installed via winget (signed package)"
        }
    }

    if (-not (Test-Path $ffmpegExe) -and -not (Test-SystemFfmpegAvailable)) {
        throw "Could not prepare local FFmpeg runtime"
    }
    Write-Ok "FFmpeg runtime ready"
    if (Get-Command Update-FlexDisplayStartupProgress -ErrorAction SilentlyContinue) {
        Update-FlexDisplayStartupProgress -Percent 25 -Message 'FFmpeg listo'
    }
}
