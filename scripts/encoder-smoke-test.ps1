# encoder-smoke-test.ps1 — quick encoder probe outside the app (2s timeout per encoder)
param(
    [int]$ProbeSeconds = 2,
    [string]$FfmpegPath = "ffmpeg"
)

$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "lib\Common.ps1")

Write-Host "FlexDisplay Encoder Smoke Test" -ForegroundColor Cyan
Write-Host "Probe timeout: ${ProbeSeconds}s per encoder`n" -ForegroundColor Gray

$encoders = Get-VendorFilteredEncoders -FfmpegPath $FfmpegPath
$results = @()

foreach ($enc in $encoders) {
    $args = @(
        "-hide_banner", "-loglevel", "error",
        "-f", "lavfi", "-i", "color=c=black:s=640x360:r=30",
        "-frames:v", "1",
        "-c:v", $enc
    )

    if ($enc -eq "h264_nvenc") { $args += @("-preset", "p1", "-bf", "0", "-zerolatency", "1") }
    if ($enc -eq "libx264") { $args += @("-preset", "ultrafast", "-tune", "zerolatency") }

    $args += @("-f", "null", "-")

    $job = Start-Job -ScriptBlock {
        param($bin, $a)
        & $bin @a 2>&1
        return $LASTEXITCODE
    } -ArgumentList $FfmpegPath, $args

    $done = Wait-Job $job -Timeout $ProbeSeconds
    if (-not $done) {
        Stop-Job $job -Force | Out-Null
        Remove-Job $job -Force | Out-Null
        $results += [PSCustomObject]@{ Encoder = $enc; Status = "TIMEOUT"; ExitCode = -1 }
        Write-Host "[TIMEOUT] $enc" -ForegroundColor Yellow
        continue
    }

    $code = Receive-Job $job
    Remove-Job $job -Force | Out-Null
    if ($code -eq 0) {
        $results += [PSCustomObject]@{ Encoder = $enc; Status = "OK"; ExitCode = 0 }
        Write-Host "[OK] $enc" -ForegroundColor Green
    } else {
        $results += [PSCustomObject]@{ Encoder = $enc; Status = "FAIL"; ExitCode = $code }
        Write-Host "[FAIL] $enc (exit $code)" -ForegroundColor Red
    }
}

$logDir = Join-Path (Split-Path -Parent $PSScriptRoot) "logs"
New-Item -ItemType Directory -Force -Path $logDir | Out-Null
$outFile = Join-Path $logDir ("encoder-smoke-{0}.txt" -f (Get-Date -Format "yyyyMMdd-HHmmss"))
$results | Format-Table -AutoSize | Out-String | Set-Content $outFile
Write-Host "`nReport saved: $outFile" -ForegroundColor Gray

if ($results | Where-Object { $_.Status -eq "OK" }) {
    exit 0
}
exit 1
