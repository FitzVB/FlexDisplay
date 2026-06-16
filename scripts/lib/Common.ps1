# FlexDisplay shared PowerShell helpers

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

function Stop-FlexDisplayHost {
    param([int]$Port = 9001)

    Stop-Process -Name "host-windows" -Force -ErrorAction SilentlyContinue
    $portOwners = Get-NetTCPConnection -LocalPort $Port -State Listen -ErrorAction SilentlyContinue |
        Select-Object -ExpandProperty OwningProcess -Unique
    foreach ($ownerPid in $portOwners) {
        if ($ownerPid -and $ownerPid -ne $PID) {
            Stop-Process -Id $ownerPid -Force -ErrorAction SilentlyContinue
        }
    }
}

function Stop-FlexDisplayGui {
    Get-CimInstance Win32_Process -Filter "Name='msedge.exe'" -ErrorAction SilentlyContinue |
        Where-Object { $_.CommandLine -like '*--app=*9001*' } |
        ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
}

function Invoke-FlexDisplayCleanup {
    param(
        [string]$AdbExe = $null,
        [int]$Port = 9001
    )

    Stop-FlexDisplayHost -Port $Port
    Stop-FlexDisplayGui
    if ($AdbExe -and (Test-Path $AdbExe)) {
        & $AdbExe kill-server 2>$null | Out-Null
    }
    Get-Job | Where-Object { $_.State -eq 'Running' } | Stop-Job -PassThru | Remove-Job -Force -ErrorAction SilentlyContinue
    Write-Host '[OK] Cleanup done.' -ForegroundColor Green
}

function Start-FlexDisplayHost {
    param(
        [string]$Root,
        [hashtable]$EnvOverrides = @{},
        [int]$Port = 9001
    )

    Stop-FlexDisplayHost -Port $Port

    foreach ($key in $EnvOverrides.Keys) {
        Set-Item -Path "Env:$key" -Value $EnvOverrides[$key]
    }

    $hostExe = Resolve-HostExePath -Root $Root
    if ($hostExe) {
        $hostDir = Split-Path -Parent $hostExe
        Set-Location $hostDir
        & $hostExe
        return $LASTEXITCODE
    }

    if (Test-Path (Join-Path $Root "host-windows\Cargo.toml")) {
        Set-Location (Join-Path $Root "host-windows")
        cargo run --release
        return $LASTEXITCODE
    }

    Write-Host "[ERROR] Host executable not found." -ForegroundColor Red
    return 1
}

function Get-VendorFilteredEncoders {
    param([string]$FfmpegPath = "ffmpeg")

    $encoders = @("h264_nvenc", "h264_qsv", "h264_amf", "libx264")
    $text = & $FfmpegPath -hide_banner -encoders 2>&1 | Out-String
    $text = $text.ToLowerInvariant()

    $gpus = Get-CimInstance Win32_VideoController -ErrorAction SilentlyContinue |
        Select-Object -ExpandProperty Name
    $gpuText = ($gpus -join " ").ToLowerInvariant()
    $hasNvidia = $gpuText -match 'nvidia|geforce|rtx|gtx|quadro'
    $hasAmd = $gpuText -match 'amd|radeon'
    $hasIntel = $gpuText -match 'intel'

    $available = @()
    foreach ($enc in $encoders) {
        if ($text -notmatch $enc) { continue }
        if ($enc -eq 'h264_nvenc' -and -not $hasNvidia) { continue }
        if ($enc -eq 'h264_amf' -and -not $hasAmd) { continue }
        if ($enc -eq 'h264_qsv' -and -not $hasIntel) { continue }
        $available += $enc
    }

    if ($available.Count -eq 0) {
        $available = @('libx264')
    }

    return $available
}
