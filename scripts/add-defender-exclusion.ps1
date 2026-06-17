#Requires -Version 5.1
#Requires -RunAsAdministrator
<#!
.SYNOPSIS
Add the FlexDisplay install folder to Microsoft Defender exclusions (false-positive mitigation).

.DESCRIPTION
Run once after extracting the ZIP. Requires Administrator approval.
Only affects Windows Defender / Microsoft Defender Antivirus.
#>
param(
    [string]$RootPath
)

$ErrorActionPreference = 'Stop'

if (-not $RootPath) {
    $RootPath = Split-Path -Parent $PSScriptRoot
}

$RootPath = (Resolve-Path -LiteralPath $RootPath).Path

if (-not (Get-Command Add-MpPreference -ErrorAction SilentlyContinue)) {
    Write-Host '[WARN] Microsoft Defender cmdlets are not available on this system.' -ForegroundColor Yellow
    Write-Host '       Add a manual exclusion in your antivirus for:' -ForegroundColor Yellow
    Write-Host "       $RootPath" -ForegroundColor Gray
    exit 1
}

$existing = @(Get-MpPreference).ExclusionPath
if ($existing -contains $RootPath) {
    Write-Host "[OK] Defender exclusion already set for:" -ForegroundColor Green
    Write-Host "     $RootPath" -ForegroundColor Gray
    exit 0
}

Add-MpPreference -ExclusionPath $RootPath
Write-Host '[OK] Microsoft Defender exclusion added for:' -ForegroundColor Green
Write-Host "     $RootPath" -ForegroundColor Gray
Write-Host ''
Write-Host 'This folder contains FlexDisplay.exe (screen streaming host).' -ForegroundColor DarkGray
Write-Host 'Unsigned open-source builds are often flagged until reputation builds up.' -ForegroundColor DarkGray
