# FlexDisplay - Wi-Fi Mode Startup

Write-Host ""
Write-Host "FlexDisplay - Wi-Fi Mode Startup" -ForegroundColor Cyan
Write-Host "====================================" -ForegroundColor Cyan
Write-Host ""

$runtimeEnv = Join-Path $PSScriptRoot "runtime-env.ps1"
if (Test-Path $runtimeEnv) {
    . $runtimeEnv -RootPath (Split-Path -Parent $PSScriptRoot)
}
. (Join-Path $PSScriptRoot "lib\Common.ps1")

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

# Register cleanup: runs when terminal window closes or PowerShell engine exits
Register-EngineEvent PowerShell.Exiting -Action {
    Invoke-FlexDisplayCleanup -Port 9001
} | Out-Null

function Invoke-Cleanup {
    Invoke-FlexDisplayCleanup -Port 9001
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
