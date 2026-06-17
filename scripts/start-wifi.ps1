# FlexDisplay - Wi-Fi Mode Startup

param(
    [switch]$Silent = $false
)

$root = Split-Path -Parent $PSScriptRoot
. (Join-Path $PSScriptRoot "lib\Common.ps1")
if ($Silent) {
    $logDir = Join-Path $root "logs"
    New-Item -ItemType Directory -Force -Path $logDir | Out-Null
    Enable-SilentFileLogging -LogFile (Join-Path $logDir "flexdisplay-start.log")
    function Write-Host {
        [CmdletBinding()]
        param(
            [Parameter(Position = 0, ValueFromPipeline = $true)]
            [object]$Object,
            [switch]$NoNewline,
            [System.ConsoleColor]$ForegroundColor,
            [System.ConsoleColor]$BackgroundColor
        )
        process {
            if ($null -ne $Object -and $script:FlexDisplayLogFile) {
                Add-Content -LiteralPath $script:FlexDisplayLogFile -Value ([string]$Object) -ErrorAction SilentlyContinue
            }
        }
    }
}

Write-Host ""
Write-Host "FlexDisplay - Wi-Fi Mode Startup" -ForegroundColor Cyan
Write-Host "====================================" -ForegroundColor Cyan
Write-Host ""

$runtimeEnv = Join-Path $PSScriptRoot "runtime-env.ps1"
if (Test-Path $runtimeEnv) {
    . $runtimeEnv -RootPath (Split-Path -Parent $PSScriptRoot)
}

# IMPORTANT: do not force localhost in Wi-Fi mode
Remove-Item Env:FLEXDISPLAY_LISTEN -ErrorAction SilentlyContinue

# Best effort: detect LAN IPv4 for user guidance
$lanIp = Get-NetIPAddress -AddressFamily IPv4 |
    Where-Object {
        $_.IPAddress -notlike '127.*' -and
        $_.IPAddress -notlike '169.254*' -and
        $_.InterfaceAlias -notmatch 'Loopback|vEthernet|Virtual|Hyper-V|VPN|Tailscale'
    } |
    Sort-Object InterfaceMetric |
    Select-Object -First 1 -ExpandProperty IPAddress

if (-not $lanIp) {
    $line = ipconfig | Select-String 'IPv4' | Select-Object -First 1
    if ($line) { $lanIp = $line.ToString().Split(':')[-1].Trim() }
}

if ($lanIp) {
    Write-Host "[OK] Wi-Fi host IP detected: $lanIp" -ForegroundColor Green
    Write-Host "[INFO] Enter this IP in the Android app to connect via Wi-Fi." -ForegroundColor Gray
} else {
    Write-Host "[WARN] Could not detect LAN IP automatically." -ForegroundColor Yellow
    Write-Host "       Run 'ipconfig' and use your IPv4 address in the Android app." -ForegroundColor Yellow
}

Write-Host ""
Write-Host "[*] Starting host on 0.0.0.0:9001 ..." -ForegroundColor Cyan

# Kill any previous instance on port 9001 before starting fresh
Stop-FlexDisplayHost -Port 9001

$root = Split-Path -Parent $PSScriptRoot
$env:FLEXDISPLAY_FPS = '60'

# Register cleanup when running in an interactive console (not silent desktop launcher).
if (-not $Silent) {
    Register-EngineEvent PowerShell.Exiting -Action {
        Invoke-FlexDisplayCleanup -Port 9001
    } | Out-Null
}

function Invoke-Cleanup {
    Invoke-FlexDisplayCleanup -Port 9001
}

$hostExe = Resolve-HostExePath -Root $root
if ($hostExe) {
    $hostDir = Split-Path -Parent $hostExe
    if ($Silent) {
        $started = Start-FlexDisplayHostDetached -Root $root -EnvOverrides @{
            FLEXDISPLAY_DISABLE_AUTO_GUI = '1'
            FLEXDISPLAY_FPS              = '60'
        }
        if (-not $started) {
            Write-Host "[ERROR] Host executable not found." -ForegroundColor Red
            exit 1
        }
        Write-Host "[OK] Host started (desktop app mode)." -ForegroundColor Green
        exit 0
    }
    Set-Location $hostDir
    try {
        & $hostExe
        $exitCode = $LASTEXITCODE
    } finally {
        Invoke-Cleanup
    }
    exit $exitCode
}

if (Test-Path (Join-Path $root "host-windows\Cargo.toml")) {
    Set-Location (Join-Path $root "host-windows")
    try {
        cargo run --release
        $exitCode = $LASTEXITCODE
    } finally {
        Invoke-Cleanup
    }
    exit $exitCode
}

Write-Host "[ERROR] Host executable not found." -ForegroundColor Red
Write-Host "        Expected one of:" -ForegroundColor Red
Write-Host "        - host-windows\target\release\host-windows.exe" -ForegroundColor DarkRed
Write-Host "        - host-windows.exe" -ForegroundColor DarkRed
exit 1
