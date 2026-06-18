# FlexDisplay shared PowerShell helpers

function Initialize-FlexDisplayWinForms {
    if ($global:FlexDisplayWinFormsLoaded) {
        return
    }

    Add-Type -AssemblyName System.Windows.Forms
    Add-Type -AssemblyName System.Drawing
    $global:FlexDisplayWinFormsLoaded = $true
}

function Start-FlexDisplayStartupSplash {
    Initialize-FlexDisplayWinForms

    if ($global:FlexDisplaySplashActive -and $global:FlexDisplaySplashForm) {
        return
    }

    $form = New-Object System.Windows.Forms.Form
    $form.Text = 'FlexDisplay'
    $form.FormBorderStyle = [System.Windows.Forms.FormBorderStyle]::FixedDialog
    $form.StartPosition = [System.Windows.Forms.FormStartPosition]::CenterScreen
    $form.ClientSize = New-Object System.Drawing.Size(500, 150)
    $form.MaximizeBox = $false
    $form.MinimizeBox = $false
    $form.ControlBox = $true
    $form.TopMost = $true
    $form.BackColor = [System.Drawing.Color]::FromArgb(14, 18, 28)
    $form.ForeColor = [System.Drawing.Color]::FromArgb(220, 230, 245)
    $form.Font = New-Object System.Drawing.Font('Segoe UI', 9)

    $title = New-Object System.Windows.Forms.Label
    $title.AutoSize = $false
    $title.SetBounds(20, 16, 460, 28)
    $title.Text = 'FlexDisplay'
    $title.Font = New-Object System.Drawing.Font('Segoe UI Semibold', 13)
    $title.ForeColor = [System.Drawing.Color]::FromArgb(120, 220, 255)
    $form.Controls.Add($title)

    $status = New-Object System.Windows.Forms.Label
    $status.AutoSize = $false
    $status.SetBounds(20, 52, 460, 40)
    $status.Text = 'Iniciando...'
    $status.ForeColor = [System.Drawing.Color]::FromArgb(200, 210, 225)
    $form.Controls.Add($status)

    $hint = New-Object System.Windows.Forms.Label
    $hint.AutoSize = $false
    $hint.SetBounds(20, 118, 460, 18)
    $hint.Text = 'La primera vez puede tardar unos minutos mientras se descargan componentes.'
    $hint.ForeColor = [System.Drawing.Color]::FromArgb(120, 130, 150)
    $hint.Font = New-Object System.Drawing.Font('Segoe UI', 8)
    $form.Controls.Add($hint)

    $progress = New-Object System.Windows.Forms.ProgressBar
    $progress.SetBounds(20, 96, 460, 18)
    $progress.Minimum = 0
    $progress.Maximum = 100
    $progress.Value = 0
    $progress.Style = [System.Windows.Forms.ProgressBarStyle]::Continuous
    $form.Controls.Add($progress)

    $global:FlexDisplaySplashForm = $form
    $global:FlexDisplaySplashStatus = $status
    $global:FlexDisplaySplashProgress = $progress
    $global:FlexDisplaySplashActive = $true

    [void]$form.Show()
    [System.Windows.Forms.Application]::DoEvents()
}

function Update-FlexDisplayStartupProgress {
    param(
        [int]$Percent = -1,
        [string]$Message = '',
        [switch]$Marquee,
        [switch]$Error
    )

    if (-not $global:FlexDisplaySplashActive -or -not $global:FlexDisplaySplashForm) {
        return
    }

    if ($Message) {
        $global:FlexDisplaySplashStatus.Text = $Message
        if ($Error) {
            $global:FlexDisplaySplashStatus.ForeColor = [System.Drawing.Color]::FromArgb(255, 140, 140)
        }
        else {
            $global:FlexDisplaySplashStatus.ForeColor = [System.Drawing.Color]::FromArgb(200, 210, 225)
        }
    }

    if ($Marquee) {
        $global:FlexDisplaySplashProgress.Style = [System.Windows.Forms.ProgressBarStyle]::Marquee
        $global:FlexDisplaySplashProgress.MarqueeAnimationSpeed = 30
    }
    elseif ($Percent -ge 0) {
        $global:FlexDisplaySplashProgress.Style = [System.Windows.Forms.ProgressBarStyle]::Continuous
        $pct = [Math]::Max(0, [Math]::Min(100, $Percent))
        if ($pct -lt $global:FlexDisplaySplashProgress.Value) {
            $global:FlexDisplaySplashProgress.Value = 0
        }
        $global:FlexDisplaySplashProgress.Value = $pct
    }

    [System.Windows.Forms.Application]::DoEvents()
}

function Complete-FlexDisplayStartupSplash {
    Update-FlexDisplayStartupProgress -Percent 100 -Message 'Listo - abriendo panel de control...'
    Start-Sleep -Milliseconds 450
    Stop-FlexDisplayStartupSplash
}

function Stop-FlexDisplayStartupSplash {
    if (-not $global:FlexDisplaySplashActive) {
        return
    }

    if ($global:FlexDisplaySplashForm) {
        $global:FlexDisplaySplashForm.Close()
        $global:FlexDisplaySplashForm.Dispose()
    }

    $global:FlexDisplaySplashForm = $null
    $global:FlexDisplaySplashStatus = $null
    $global:FlexDisplaySplashProgress = $null
    $global:FlexDisplaySplashActive = $false
}

function Enable-SilentFileLogging {
    param([string]$LogFile)
    $script:FlexDisplayLogFile = $LogFile
}

function Wait-FlexDisplayHostReady {
    param(
        [int]$Port = 9001,
        [int]$TimeoutSec = 30
    )

    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    while ((Get-Date) -lt $deadline) {
        if (Get-NetTCPConnection -LocalPort $Port -State Listen -ErrorAction SilentlyContinue) {
            return $true
        }
        Start-Sleep -Milliseconds 400
    }
    return $false
}

function Open-FlexDisplayGui {
    param(
        [string]$Root,
        [string]$HostIp = '127.0.0.1',
        [int]$Port = 9001
    )

    $url = "http://${HostIp}:${Port}"
    $candidates = @(
        (Join-Path ${env:ProgramFiles(x86)} 'Microsoft\Edge\Application\msedge.exe'),
        (Join-Path $env:ProgramFiles 'Microsoft\Edge\Application\msedge.exe'),
        (Join-Path $env:ProgramFiles 'Google\Chrome\Application\chrome.exe'),
        (Join-Path ${env:ProgramFiles(x86)} 'Google\Chrome\Application\chrome.exe')
    )

    foreach ($browser in $candidates) {
        if (Test-Path -LiteralPath $browser) {
            Start-Process -FilePath $browser -ArgumentList "--app=$url" -WindowStyle Normal | Out-Null
            return [PSCustomObject]@{ Launched = $true; Port = $Port; Browser = $browser }
        }
    }

    $openGuiVbs = Join-Path $Root 'FlexDisplay-OpenGui.vbs'
    if (Test-Path -LiteralPath $openGuiVbs) {
        Start-Process wscript.exe -ArgumentList @('//B', $openGuiVbs) | Out-Null
        return [PSCustomObject]@{ Launched = $true; Port = $Port }
    }

    Start-Process $url | Out-Null
    return $null
}

function Wait-FlexDisplayGuiClose {
    param(
        [int]$Port = 9001,
        [int]$GraceSec = 8
    )

    Start-Sleep -Seconds $GraceSec
    while ($true) {
        $running = Get-CimInstance Win32_Process -Filter "Name='msedge.exe' OR Name='chrome.exe'" -ErrorAction SilentlyContinue |
            Where-Object { $_.CommandLine -like "*--app=*${Port}*" }
        if (-not $running) {
            return
        }
        Start-Sleep -Seconds 2
    }
}

function Resolve-HostExePath {
    param([string]$Root)

    $candidates = @(
        (Join-Path $Root "FlexDisplay.exe"),
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

function Stop-FlexDisplayProcessTree {
    param([int]$ProcessId)

    if (-not $ProcessId -or $ProcessId -eq $PID) {
        return
    }

    Get-CimInstance Win32_Process -Filter "ParentProcessId=$ProcessId" -ErrorAction SilentlyContinue |
        ForEach-Object { Stop-FlexDisplayProcessTree -ProcessId $_.ProcessId }

    Stop-Process -Id $ProcessId -Force -ErrorAction SilentlyContinue
}

function Stop-FlexDisplayHost {
    param([int]$Port = 9001)

    foreach ($procName in @('FlexDisplay', 'host-windows')) {
        Get-Process -Name $procName -ErrorAction SilentlyContinue |
            ForEach-Object { Stop-FlexDisplayProcessTree -ProcessId $_.Id }
    }

    $portOwners = Get-NetTCPConnection -LocalPort $Port -State Listen -ErrorAction SilentlyContinue |
        Select-Object -ExpandProperty OwningProcess -Unique
    foreach ($ownerPid in $portOwners) {
        Stop-FlexDisplayProcessTree -ProcessId $ownerPid
    }
}

function Stop-FlexDisplayGui {
    Get-CimInstance Win32_Process -Filter "Name='msedge.exe' OR Name='chrome.exe'" -ErrorAction SilentlyContinue |
        Where-Object { $_.CommandLine -like '*--app=*9001*' } |
        ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
}

function Stop-FlexDisplayAdbSidecars {
    Get-CimInstance Win32_Process -Filter "Name='adb.exe'" -ErrorAction SilentlyContinue |
        Where-Object { $_.CommandLine -like '*logcat*' -or $_.CommandLine -like '*reverse*' } |
        ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
}

function Invoke-FlexDisplayCleanup {
    param(
        [string]$AdbExe = $null,
        [int]$Port = 9001
    )

    Stop-FlexDisplayHost -Port $Port
    Stop-FlexDisplayGui
    Stop-FlexDisplayAdbSidecars

    if ($AdbExe -and (Test-Path $AdbExe)) {
        & $AdbExe reverse --remove-all 2>$null | Out-Null
        & $AdbExe kill-server 2>$null | Out-Null
    }
    else {
        $adbCmd = Get-Command adb -ErrorAction SilentlyContinue
        if ($adbCmd) {
            & $adbCmd.Source reverse --remove-all 2>$null | Out-Null
            & $adbCmd.Source kill-server 2>$null | Out-Null
        }
    }

    Get-Job | Where-Object { $_.State -eq 'Running' } | Stop-Job -PassThru | Remove-Job -Force -ErrorAction SilentlyContinue
    Write-Host '[OK] FlexDisplay host, GUI, and ADB stopped.' -ForegroundColor Green
}

function Start-FlexDisplayLogcatCapture {
    param(
        [string]$AdbPath,
        [string]$Serial,
        [string]$Root
    )

    if (-not $AdbPath -or -not $Serial) {
        return $null
    }

    $logDir = Join-Path $Root 'logs'
    New-Item -ItemType Directory -Force -Path $logDir | Out-Null
    $logcatFile = Join-Path $logDir "logcat-$Serial.txt"

    & $AdbPath -s $Serial logcat -c 2>$null | Out-Null

    Start-Process -FilePath $AdbPath `
        -ArgumentList @('-s', $Serial, 'logcat', '-v', 'time', '*:W', 'H264Decoder:V', 'MainActivity:V') `
        -RedirectStandardOutput $logcatFile `
        -RedirectStandardError "$logcatFile.err" `
        -WindowStyle Hidden | Out-Null

    return $logcatFile
}

function Start-FlexDisplayHostDetached {
    param(
        [string]$Root,
        [hashtable]$EnvOverrides = @{}
    )

    foreach ($key in $EnvOverrides.Keys) {
        Set-Item -Path "Env:$key" -Value $EnvOverrides[$key]
    }

    $hostExe = Resolve-HostExePath -Root $Root
    if (-not $hostExe) {
        return $false
    }

    $hostDir = Split-Path -Parent $hostExe
    Start-Process -FilePath $hostExe -WorkingDirectory $hostDir | Out-Null
    return $true
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
