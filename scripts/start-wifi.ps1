# FlexDisplay - Wi-Fi Mode Startup

Write-Host ""
Write-Host "FlexDisplay - Wi-Fi Mode Startup" -ForegroundColor Cyan
Write-Host "====================================" -ForegroundColor Cyan
Write-Host ""

$runtimeEnv = Join-Path $PSScriptRoot "runtime-env.ps1"
if (Test-Path $runtimeEnv) {
    . $runtimeEnv -RootPath (Split-Path -Parent $PSScriptRoot)
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

# IMPORTANT: do not force localhost in Wi-Fi mode
Remove-Item Env:TABLET_MONITOR_LISTEN -ErrorAction SilentlyContinue

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
Stop-Process -Name "host-windows" -Force -ErrorAction SilentlyContinue
$portOwners = Get-NetTCPConnection -LocalPort 9001 -State Listen -ErrorAction SilentlyContinue |
    Select-Object -ExpandProperty OwningProcess -Unique
foreach ($ownerPid in $portOwners) {
    if ($ownerPid -and $ownerPid -ne $PID) {
        Stop-Process -Id $ownerPid -Force -ErrorAction SilentlyContinue
    }
}

$root = Split-Path -Parent $PSScriptRoot
$env:TABLET_MONITOR_FPS = '60'

# Register cleanup: runs when terminal window closes or PowerShell engine exits
Register-EngineEvent PowerShell.Exiting -Action {
    Stop-Process -Name 'host-windows' -Force -ErrorAction SilentlyContinue
    Get-CimInstance Win32_Process -Filter "Name='msedge.exe'" -ErrorAction SilentlyContinue |
        Where-Object { $_.CommandLine -like '*--app=*9001*' } |
        ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
} | Out-Null

function Invoke-Cleanup {
    Stop-Process -Name 'host-windows' -Force -ErrorAction SilentlyContinue
    Get-CimInstance Win32_Process -Filter "Name='msedge.exe'" -ErrorAction SilentlyContinue |
        Where-Object { $_.CommandLine -like '*--app=*9001*' } |
        ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
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
