#Requires -Version 5.1
<#!
.SYNOPSIS
Simple release entry point for open-source/internal distribution.

.DESCRIPTION
Runs scripts/package.ps1 with sensible defaults so release creation
is a single command for maintainers.

.PARAMETER Version
Optional package version override.

.PARAMETER SkipAndroid
Do not include Android APK.

.PARAMETER NoBuildAndroid
Do not build Android APK before packaging.

.PARAMETER BundleRuntime
Include ADB/FFmpeg inside the ZIP package.
#>
param(
    [string]$Version = "",
    [switch]$SkipAndroid,
    [switch]$NoBuildAndroid,
    [switch]$BundleRuntime
)

$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$packageScript = Join-Path $PSScriptRoot "package.ps1"

if (-not (Test-Path $packageScript)) {
    throw "scripts/package.ps1 not found"
}

$packageArgs = @{}
if ($Version) {
    $packageArgs.Version = $Version
}
if ($SkipAndroid) {
    $packageArgs.SkipAndroid = $true
}
elseif (-not $NoBuildAndroid) {
    # Default: fresh debug APK in every release package.
    $packageArgs.BuildAndroid = $true
}
if ($BundleRuntime) {
    $packageArgs.SkipBundledRuntime = $false
}

Write-Host ""
Write-Host "FlexDisplay - Release Package" -ForegroundColor Cyan
Write-Host "============================" -ForegroundColor Cyan
Write-Host ""

& $packageScript @packageArgs
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$distVersion = if ($Version) { $Version } else {
    $cargoToml = Join-Path $repoRoot "host-windows\Cargo.toml"
    (Select-String -Path $cargoToml -Pattern 'version\s*=\s*"([^"]+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
}
$distApk = Join-Path $repoRoot "dist\FlexDisplay-v$distVersion-windows-lite\FlexDisplay.apk"
if (-not $SkipAndroid -and -not (Test-Path -LiteralPath $distApk)) {
    throw "Release package is missing FlexDisplay.apk at $distApk"
}
