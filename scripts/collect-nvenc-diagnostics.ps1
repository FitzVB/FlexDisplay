#Requires -Version 5.1
param(
    [string]$OutputDir = ""
)

$ErrorActionPreference = "Continue"
$ProgressPreference = "SilentlyContinue"

$repoRoot = Split-Path -Parent $PSScriptRoot
if (-not $OutputDir) {
    $OutputDir = Join-Path $repoRoot "logs"
}
if (-not (Test-Path $OutputDir)) {
    New-Item -ItemType Directory -Path $OutputDir -Force | Out-Null
}

$stamp = Get-Date -Format "yyyyMMdd-HHmmss"
$logPath = Join-Path $OutputDir "nvenc-diagnostic-$stamp.txt"

function Write-Log {
    param([string]$Text)
    $Text | Out-File -FilePath $logPath -Append -Encoding UTF8
}

function Write-Section {
    param([string]$Title)
    Write-Log ""
    Write-Log "============================================================"
    Write-Log $Title
    Write-Log "============================================================"
}

function Run-Cmd {
    param(
        [string]$Title,
        [string]$FilePath,
        [string[]]$CmdArgs
    )

    Write-Section $Title
    Write-Log "COMMAND: $FilePath $($CmdArgs -join ' ')"

    try {
        $output = & $FilePath @CmdArgs 2>&1
        if ($output) {
            $output | ForEach-Object { Write-Log "$_" }
        }
        Write-Log "EXIT_CODE: $LASTEXITCODE"
    } catch {
        Write-Log "ERROR: $($_.Exception.Message)"
    }
}

function Resolve-Ffmpeg {
    $candidates = @(
        (Join-Path $repoRoot ".runtime\ffmpeg\bin\ffmpeg.exe"),
        (Join-Path $repoRoot "ffmpeg.exe")
    )

    foreach ($c in $candidates) {
        if (Test-Path $c) {
            return $c
        }
    }

    $cmd = Get-Command ffmpeg -ErrorAction SilentlyContinue
    if ($cmd) {
        return $cmd.Source
    }

    return $null
}

Write-Section "Context"
Write-Log "Timestamp: $(Get-Date -Format o)"
Write-Log "RepoRoot: $repoRoot"
Write-Log "Host: $env:COMPUTERNAME"
Write-Log "User: $env:USERNAME"
Write-Log "PSVersion: $($PSVersionTable.PSVersion)"
Write-Log "TABLET_MONITOR_FFMPEG: $env:TABLET_MONITOR_FFMPEG"

Write-Section "OS"
try {
    Get-ComputerInfo | Select-Object WindowsProductName, WindowsVersion, OsBuildNumber, OsHardwareAbstractionLayer |
        Format-List | Out-String -Width 220 | ForEach-Object { Write-Log $_ }
} catch {
    Write-Log "Get-ComputerInfo failed: $($_.Exception.Message)"
}

Write-Section "GPU"
try {
    Get-CimInstance Win32_VideoController |
        Select-Object Name, DriverVersion, AdapterRAM, VideoProcessor |
        Format-Table -AutoSize | Out-String -Width 220 | ForEach-Object { Write-Log $_ }
} catch {
    Write-Log "Get-CimInstance Win32_VideoController failed: $($_.Exception.Message)"
}

$nvidiaSmi = Get-Command nvidia-smi -ErrorAction SilentlyContinue
if ($nvidiaSmi) {
    Run-Cmd -Title "nvidia-smi" -FilePath $nvidiaSmi.Source -CmdArgs @()
    Run-Cmd -Title "nvidia-smi query" -FilePath $nvidiaSmi.Source -CmdArgs @("--query-gpu=name,driver_version,vbios_version,pstate", "--format=csv,noheader")
} else {
    Write-Section "nvidia-smi"
    Write-Log "nvidia-smi not found in PATH"
}

$ffmpeg = Resolve-Ffmpeg
Write-Section "FFmpeg resolve"
if ($ffmpeg) {
    Write-Log "Using ffmpeg: $ffmpeg"
} else {
    Write-Log "ffmpeg not found"
}

if ($ffmpeg) {
    Run-Cmd -Title "ffmpeg -version" -FilePath $ffmpeg -CmdArgs @("-hide_banner", "-version")
    Run-Cmd -Title "ffmpeg -encoders (nvenc/qsv/amf/x264)" -FilePath $ffmpeg -CmdArgs @("-hide_banner", "-encoders")
    Run-Cmd -Title "ffmpeg -h encoder=h264_nvenc" -FilePath $ffmpeg -CmdArgs @("-hide_banner", "-h", "encoder=h264_nvenc")

    Run-Cmd -Title "NVENC smoke test (minimal)" -FilePath $ffmpeg -CmdArgs @(
        "-hide_banner",
        "-loglevel", "verbose",
        "-f", "lavfi",
        "-i", "color=size=1280x720:rate=60:color=black",
        "-frames:v", "120",
        "-an",
        "-c:v", "h264_nvenc",
        "-preset", "p4",
        "-f", "null",
        "NUL"
    )

    Run-Cmd -Title "NVENC smoke test (app-like flags)" -FilePath $ffmpeg -CmdArgs @(
        "-hide_banner",
        "-loglevel", "verbose",
        "-f", "lavfi",
        "-i", "color=size=1280x720:rate=60:color=black",
        "-an",
        "-r", "60",
        "-c:v", "h264_nvenc",
        "-preset", "p1",
        "-tune", "ll",
        "-rc", "cbr",
        "-bf", "0",
        "-zerolatency", "1",
        "-level", "5.1",
        "-g", "30",
        "-b:v", "10000k",
        "-maxrate", "10000k",
        "-bufsize", "2500k",
        "-frames:v", "120",
        "-f", "null",
        "NUL"
    )
}

Write-Section "Done"
Write-Log "Diagnostic log written to: $logPath"
Write-Host "NVENC diagnostic complete: $logPath" -ForegroundColor Green
