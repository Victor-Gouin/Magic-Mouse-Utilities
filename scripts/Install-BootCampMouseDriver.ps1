<#
.SYNOPSIS
    Assistant d'installation et de reparation des pilotes Apple Magic Mouse (1, 2 Lightning & USB-C) sur Windows.
#>

param(
    [switch]$CheckOnly,
    [switch]$AutoInstall,
    [switch]$Elevated,
    [string]$DriverInfPath
)

$ErrorActionPreference = "Continue"

function Test-IsAdmin {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = New-Object Security.Principal.WindowsPrincipal($identity)
    return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

Write-Host "==================================================================" -ForegroundColor Cyan
Write-Host "   Assistant Pilotes Apple Magic Mouse (Magic Mouse Studio)" -ForegroundColor Cyan
Write-Host "==================================================================" -ForegroundColor Cyan

# 1. Recherche des peripheriques Apple Magic Mouse dans le gestionnaire PnP
$appleDevices = Get-PnpDevice -ErrorAction SilentlyContinue | Where-Object {
    $_.InstanceId -match "VID_05AC" -or
    $_.InstanceId -match "VID_004C" -or
    $_.InstanceId -match "05AC" -or
    $_.InstanceId -match "004C" -or
    $_.FriendlyName -match "Magic Mouse" -or
    $_.FriendlyName -match "Apple Wireless Mouse"
}

if ($appleDevices) {
    Write-Host "`n[+] Peripheriques Apple detectes dans Windows PnP :" -ForegroundColor Green
    $appleDevices | Select-Object Status, Class, FriendlyName, InstanceId | Format-Table -AutoSize
} else {
    Write-Host "`n[!] Aucune Magic Mouse detectee actuellement dans le gestionnaire de peripheriques." -ForegroundColor Yellow
}

if ($CheckOnly) {
    exit 0
}

# 2. Elevation Administrateur automatique si necessaire pour pnputil / redemarrage PnP
if (-not (Test-IsAdmin)) {
    Write-Host "`n[*] Demande des privileges Administrateur (UAC) pour installer/reinitialiser les pilotes..." -ForegroundColor Cyan
    $scriptPath = $MyInvocation.MyCommand.Definition
    $argList = "-NoProfile -ExecutionPolicy Bypass -File `"$scriptPath`" -AutoInstall -Elevated"
    if ($DriverInfPath) {
        $argList += " -DriverInfPath `"$DriverInfPath`""
    }
    try {
        $proc = Start-Process powershell.exe -ArgumentList $argList -Verb RunAs -Wait -PassThru
        exit $proc.ExitCode
    } catch {
        Write-Host "[!] Elevation Administrateur annulee ou refusee par l'utilisateur." -ForegroundColor Red
        exit 1
    }
}

$projectRoot = Split-Path -Parent $PSScriptRoot
$driversDir = Join-Path $projectRoot "drivers"
if (-not (Test-Path $driversDir)) {
    New-Item -ItemType Directory -Path $driversDir -Force | Out-Null
}

# 3. Installation d'un fichier .inf specifique ou de tous les .inf presents dans ./drivers
$infFiles = @()
if ($DriverInfPath -and (Test-Path $DriverInfPath)) {
    $infFiles += Get-Item $DriverInfPath
} else {
    $infFiles += Get-ChildItem -Path $driversDir -Filter "*.inf" -Recurse -ErrorAction SilentlyContinue
}

if ($infFiles.Count -gt 0) {
    foreach ($inf in $infFiles) {
        Write-Host "`n[*] Installation du pilote INF via pnputil : $($inf.FullName)" -ForegroundColor Cyan
        & pnputil.exe /add-driver "$($inf.FullName)" /install
    }
}

# 4. Reactivation et redemarrage des noeuds PnP Apple Magic Mouse
if ($appleDevices) {
    Write-Host "`n[*] Reinitialisation des collections HID Apple Magic Mouse..." -ForegroundColor Cyan
    foreach ($dev in $appleDevices) {
        if ($dev.Status -ne "OK") {
            Enable-PnpDevice -InstanceId $dev.InstanceId -Confirm:$false -ErrorAction SilentlyContinue
        }
        if ($dev.Class -eq "HIDClass" -or $dev.Class -eq "Mouse") {
            & pnputil.exe /restart-device "$($dev.InstanceId)" | Out-Null
        }
    }
}

& pnputil.exe /scan-devices | Out-Null
Write-Host "`n[+] Operation terminee avec succes !" -ForegroundColor Green
exit 0
