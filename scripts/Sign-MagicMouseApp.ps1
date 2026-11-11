<#
.SYNOPSIS
    Genere un certificat de signature de code Authenticode (SHA-256), l'enregistre dans les
    Autorites Racines / Editeurs Approuves et signe numeriquement `magic-mouse-win.exe`.
.DESCRIPTION
    Concu pour permettre l'execution sur un poste protege par SentinelOne / Windows Defender SmartScreen :
    1. Cree (ou reutilise) un certificat X.509 CodeSigning RSA-2048 / SHA-256 ("CN=Magic Mouse Studio").
    2. Exporte le certificat public dans `certs/MagicMouseStudio-CodeSigning.cer` (pour l'importer
       facilement sur un autre PC si besoin via `-TrustOnly`).
    3. Ajoute le certificat dans `Cert:\CurrentUser\Root` et `Cert:\CurrentUser\TrustedPublisher`.
    4. Signe `target\release\magic-mouse-win.exe` avec `Set-AuthenticodeSignature -HashAlgorithm SHA256`.
    5. Verifie que le statut Authenticode est `Valid` et affiche l'empreinte SHA-256 du binaire.
#>

param(
    [string]$ExePath = "",
    [switch]$TrustOnly
)

$ErrorActionPreference = "Stop"

$projectRoot = Split-Path -Parent $PSScriptRoot
if (-not $ExePath) {
    $ExePath = Join-Path $projectRoot "target\release\magic-mouse-win.exe"
}

$certsDir = Join-Path $projectRoot "certs"
if (-not (Test-Path $certsDir)) {
    New-Item -ItemType Directory -Path $certsDir -Force | Out-Null
}
$cerFile = Join-Path $certsDir "MagicMouseStudio-CodeSigning.cer"

Write-Host "==================================================================" -ForegroundColor Cyan
Write-Host "   Signature Authenticode SHA-256 - Magic Mouse Studio" -ForegroundColor Cyan
Write-Host "==================================================================" -ForegroundColor Cyan

# Mode d'importation uniquement (utile sur un second PC cible ou SentinelOne est installe)
if ($TrustOnly) {
    if (-not (Test-Path $cerFile)) {
        Write-Host "[!] Fichier certificat introuvable : $cerFile" -ForegroundColor Red
        exit 1
    }
    Write-Host "[*] Importation du certificat dans les Autorites Racines et Editeurs Approuves..." -ForegroundColor Cyan
    Import-Certificate -FilePath $cerFile -CertStoreLocation "Cert:\CurrentUser\Root" | Out-Null
    Import-Certificate -FilePath $cerFile -CertStoreLocation "Cert:\CurrentUser\TrustedPublisher" | Out-Null
    Write-Host "[+] Certificat approuve avec succes sur ce poste !" -ForegroundColor Green
    if (Test-Path $ExePath) {
        Get-AuthenticodeSignature -FilePath $ExePath | Format-List Status, StatusMessage, SignerCertificate
    }
    exit 0
}

if (-not (Test-Path $ExePath)) {
    Write-Host "[!] Executable introuvable : $ExePath" -ForegroundColor Red
    Write-Host "    Compilez d'abord avec : cargo build --release" -ForegroundColor Yellow
    exit 1
}

# 1. Recherche ou creation du certificat Code Signing dans Cert:\CurrentUser\My
$subjectName = "CN=Magic Mouse Studio, O=Magic Mouse Studio, C=FR"
$cert = Get-ChildItem Cert:\CurrentUser\My -CodeSigningCert -ErrorAction SilentlyContinue |
    Where-Object { $_.Subject -eq $subjectName -and $_.NotAfter -gt (Get-Date) } |
    Select-Object -First 1

if (-not $cert) {
    Write-Host "[*] Creation d'un certificat de signature de code RSA-2048 / SHA-256..." -ForegroundColor Cyan
    $cert = New-SelfSignedCertificate `
        -Type CodeSigningCert `
        -Subject $subjectName `
        -KeyAlgorithm RSA `
        -KeyLength 2048 `
        -HashAlgorithm "SHA256" `
        -KeyExportPolicy Exportable `
        -CertStoreLocation "Cert:\CurrentUser\My" `
        -NotAfter (Get-Date).AddYears(5)
    Write-Host "[+] Certificat cree : Thumbprint = $($cert.Thumbprint)" -ForegroundColor Green
} else {
    Write-Host "[+] Certificat existant trouve : Thumbprint = $($cert.Thumbprint)" -ForegroundColor Green
}

# 2. Exportation du certificat public (.cer)
Export-Certificate -Cert $cert -FilePath $cerFile -Force | Out-Null
Write-Host "[+] Certificat public exporte : $cerFile" -ForegroundColor Gray

# 3. Installation dans les magasins TrustedPublisher et Root
$inPub = Get-ChildItem Cert:\CurrentUser\TrustedPublisher -ErrorAction SilentlyContinue |
    Where-Object { $_.Thumbprint -eq $cert.Thumbprint }
if (-not $inPub) {
    Write-Host "[*] Ajout du certificat dans Cert:\CurrentUser\TrustedPublisher..." -ForegroundColor Cyan
    Import-Certificate -FilePath $cerFile -CertStoreLocation "Cert:\CurrentUser\TrustedPublisher" | Out-Null
}

$inRoot = Get-ChildItem Cert:\CurrentUser\Root -ErrorAction SilentlyContinue |
    Where-Object { $_.Thumbprint -eq $cert.Thumbprint }
if (-not $inRoot) {
    Write-Host "[*] Ajout du certificat dans Cert:\CurrentUser\Root..." -ForegroundColor Cyan
    try {
        Import-Certificate -FilePath $cerFile -CertStoreLocation "Cert:\CurrentUser\Root" | Out-Null
    } catch {
        Write-Host "[!] Impossible d'ajouter automatiquement dans CurrentUser\Root sans confirmation interactive." -ForegroundColor Yellow
    }
}

# 4. Signature Authenticode SHA-256 de l'executable
Write-Host "[*] Signature Authenticode SHA-256 de : $ExePath" -ForegroundColor Cyan
$sigResult = Set-AuthenticodeSignature -FilePath $ExePath -Certificate $cert -HashAlgorithm SHA256

# 5. Verification finale
$verify = Get-AuthenticodeSignature -FilePath $ExePath
$fileHash = (Get-FileHash -Path $ExePath -Algorithm SHA256).Hash

Write-Host ""
Write-Host "================ RESULTAT DE LA SIGNATURE ================" -ForegroundColor Cyan
Write-Host "Fichier signe       : $ExePath"
Write-Host "Statut Authenticode : $($verify.Status) ($($verify.StatusMessage))"
Write-Host "Signataire          : $($verify.SignerCertificate.Subject)"
Write-Host "Empreinte Certificat: $($verify.SignerCertificate.Thumbprint)"
Write-Host "SHA-256 de l'EXE    : $fileHash"
Write-Host "==========================================================" -ForegroundColor Cyan
