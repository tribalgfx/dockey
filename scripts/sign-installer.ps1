<#
.SYNOPSIS
  Ondertekent Dockey-Setup-<versie>.exe voor de updatefunctie van Dockey, of maakt eenmalig het sleutelpaar.

.DESCRIPTION
  Dockey installeert een update alleen als de installer ondertekend is met de privesleutel die hoort bij
  src-tauri\update-key.pub (ECDSA P-256, SHA-256). Zo is een gestolen GitHub-account of -token niet genoeg om
  iedereen een andere installer te laten draaien.

  De privesleutel staat buiten de repo (standaard %USERPROFILE%\.dockey\update-signing-key.json) en hoort
  nooit op GitHub of in een CI-geheim: wie hem heeft, kan updates maken die elke Dockey installeert. Bewaar een
  kopie op een veilige plek (bijv. je wachtwoordmanager). Zonder die sleutel kunnen bestaande installaties
  niet meer automatisch bijwerken; dan moet iedereen eenmalig zelf een nieuwe installer draaien.

  Werkt in Windows PowerShell 5.1 (.NET Framework 4.7 of nieuwer); niets te installeren.

.EXAMPLE
  .\scripts\sign-installer.ps1 -NewKey
  Maakt het sleutelpaar (eenmalig) en schrijft de publieke sleutel naar src-tauri\update-key.pub.

.EXAMPLE
  .\scripts\sign-installer.ps1 artifacts\Dockey-Setup-0.6.0.exe
  Schrijft Dockey-Setup-0.6.0.exe.sig ernaast. Dat bestand gaat als extra bestand bij de release.
#>
param(
    [Parameter(Position = 0)][string]$Installer,
    [switch]$NewKey,
    [string]$KeyFile = (Join-Path $env:USERPROFILE '.dockey\update-signing-key.json'),
    [string]$PublicKeyFile = (Join-Path (Split-Path -Parent $PSScriptRoot) 'src-tauri\update-key.pub')
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Core
$curve = [System.Security.Cryptography.ECCurve+NamedCurves]::nistP256
$sha256 = [System.Security.Cryptography.HashAlgorithmName]::SHA256

function ConvertTo-PublicKey($x, $y) {
    [Convert]::ToBase64String([byte[]]($x + $y))
}

if ($NewKey) {
    if (Test-Path $KeyFile) {
        throw "Er is al een sleutel ($KeyFile). Een nieuwe maken zou bestaande installaties hun updates kosten."
    }
    $ec = [System.Security.Cryptography.ECDsa]::Create($curve)
    $p = $ec.ExportParameters($true)
    New-Item -ItemType Directory -Force (Split-Path $KeyFile) | Out-Null
    [ordered]@{
        curve = 'P-256'
        d     = [Convert]::ToBase64String($p.D)
        x     = [Convert]::ToBase64String($p.Q.X)
        y     = [Convert]::ToBase64String($p.Q.Y)
    } | ConvertTo-Json | Set-Content -Encoding ascii -Path $KeyFile
    # Alleen deze gebruiker mag het bestand lezen.
    $sid = [System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value
    & icacls $KeyFile /inheritance:r /grant:r "*${sid}:(R,W)" | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'Rechten op het sleutelbestand zetten is mislukt.' }
    Set-Content -Encoding ascii -NoNewline -Path $PublicKeyFile -Value (ConvertTo-PublicKey $p.Q.X $p.Q.Y)
    Write-Host "Privesleutel: $KeyFile (maak er een veilige kopie van; nooit in git)"
    Write-Host "Publieke sleutel: $PublicKeyFile (die hoort wel in git)"
    return
}

if (-not $Installer) { throw 'Geef de installer op, of gebruik -NewKey.' }
if (-not (Test-Path $KeyFile)) { throw "Geen sleutel gevonden in $KeyFile (zie -NewKey)." }
$key = Get-Content $KeyFile -Raw | ConvertFrom-Json

$q = New-Object System.Security.Cryptography.ECPoint
$q.X = [Convert]::FromBase64String($key.x)
$q.Y = [Convert]::FromBase64String($key.y)
$params = New-Object System.Security.Cryptography.ECParameters
$params.Curve = $curve
$params.D = [Convert]::FromBase64String($key.d)
$params.Q = $q

# De sleutel moet horen bij de publieke sleutel die in Dockey zit, anders weigert Dockey de update.
$expected = (Get-Content $PublicKeyFile -Raw).Trim()
if ((ConvertTo-PublicKey $q.X $q.Y) -ne $expected) {
    throw "Deze sleutel hoort niet bij $PublicKeyFile."
}

$path = (Resolve-Path $Installer).Path
$bytes = [System.IO.File]::ReadAllBytes($path)
$ec = [System.Security.Cryptography.ECDsa]::Create($params)
$signature = $ec.SignData($bytes, $sha256)
if ($signature.Length -ne 64 -or -not $ec.VerifyData($bytes, $signature, $sha256)) {
    throw 'Ondertekenen is mislukt (controle klopt niet).'
}
Set-Content -Encoding ascii -NoNewline -Path "$path.sig" -Value ([Convert]::ToBase64String($signature))

$name = Split-Path -Leaf $path
$version = [regex]::Match($name, 'Dockey-Setup-(.+)\.exe$').Groups[1].Value
Write-Host "Klaar: $path.sig"
Write-Host "Bij de release zetten: gh release upload v$version `"$path.sig`""
