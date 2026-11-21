<#
.SYNOPSIS
  Bouwt Dockey-Setup-<versie>.exe: een installer die zonder beheerdersrechten werkt.

.DESCRIPTION
  1. Bouwt dockey.exe met cargo (release, --locked).
  2. Laat Inno Setup (installer\Dockey.iss) er een Setup.exe van maken in artifacts\.

  Nodig: Rust (rustup) met de Visual Studio C++ Build Tools, en Inno Setup 6. Ontbreekt Inno Setup, dan
  installeert dit script het met winget (per gebruiker).

  De installer is niet digitaal ondertekend; Windows kan daarom een SmartScreen-waarschuwing tonen
  ("Meer info" -> "Toch uitvoeren").

.EXAMPLE
  .\scripts\build-installer.ps1
#>
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot

function Find-InnoSetup {
    @(
        (Join-Path $env:LOCALAPPDATA 'Programs\Inno Setup 6\ISCC.exe'),
        'C:\Program Files (x86)\Inno Setup 6\ISCC.exe',
        'C:\Program Files\Inno Setup 6\ISCC.exe'
    ) | Where-Object { Test-Path $_ } | Select-Object -First 1
}

function Ensure-InnoSetup {
    $iscc = Find-InnoSetup
    if ($iscc) { return $iscc }
    if (-not (Get-Command winget -ErrorAction SilentlyContinue)) {
        throw 'Inno Setup ontbreekt en winget ook. Installeer Inno Setup 6 zelf (https://jrsoftware.org/isdl.php) en start dit script opnieuw.'
    }
    Write-Host 'Inno Setup ontbreekt. Installeren met winget (per gebruiker) ...'
    & winget install --id JRSoftware.InnoSetup -e --scope user --accept-package-agreements --accept-source-agreements
    if ($LASTEXITCODE -ne 0) { throw "Installeren van Inno Setup met winget is mislukt (code $LASTEXITCODE)." }
    $iscc = Find-InnoSetup
    if (-not $iscc) { throw 'Inno Setup is geinstalleerd, maar ISCC.exe is niet gevonden.' }
    return $iscc
}

# cargo staat na een verse rustup-installatie nog niet in het PATH van deze sessie.
$cargo = Get-Command cargo -ErrorAction SilentlyContinue
if (-not $cargo) {
    $candidate = Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe'
    if (-not (Test-Path $candidate)) { throw 'cargo ontbreekt. Installeer Rust via https://rustup.rs (met de Visual Studio C++ Build Tools).' }
    $cargo = $candidate
}
$iscc = Ensure-InnoSetup

# De versie komt uit tauri.conf.json.
$version = (Get-Content (Join-Path $repo 'src-tauri\tauri.conf.json') -Raw | ConvertFrom-Json).version
Write-Host "Versie $version"

Write-Host 'dockey.exe bouwen (release) ...'
Push-Location (Join-Path $repo 'src-tauri')
# cargo meldt zijn voortgang op stderr; Windows PowerShell 5.1 ziet dat bij 'Stop' als fout.
$ErrorActionPreference = 'Continue'
try {
    & $cargo build --release --locked
    if ($LASTEXITCODE -ne 0) { throw 'Bouwen van dockey.exe is mislukt.' }
} finally {
    $ErrorActionPreference = 'Stop'
    Pop-Location
}
$exe = Join-Path $repo 'src-tauri\target\release\dockey.exe'

Write-Host 'Installer bouwen ...'
& $iscc "/DAppVersion=$version" "/DSourceExe=$exe" (Join-Path $repo 'installer\Dockey.iss')
if ($LASTEXITCODE -ne 0) { throw 'Inno Setup meldde een fout.' }

$setup = Join-Path $repo "artifacts\Dockey-Setup-$version.exe"
$size = [math]::Round((Get-Item $setup).Length / 1MB, 1)
Write-Host "Klaar: $setup ($size MB)"
