# Usage: .\build.ps1
#
# Builds the webcloner graphical interface (Tauri app).
# Requires Rust + Tauri CLI. If missing, run: .\setup.ps1

$ErrorActionPreference = "Stop"
$Here = Split-Path -Parent $MyInvocation.MyCommand.Path

function Get-CargoExe {
    if (Get-Command cargo -ErrorAction SilentlyContinue) {
        return (Get-Command cargo).Source
    }
    $userCargo = Join-Path $env:USERPROFILE ".cargo\bin\cargo.exe"
    if (Test-Path $userCargo) {
        return $userCargo
    }
    return $null
}

function Ensure-TauriCli([string]$CargoExe) {
    $cargoBin = Split-Path -Parent $CargoExe
    $tauriExe = Join-Path $cargoBin "cargo-tauri.exe"
    if (Test-Path $tauriExe) {
        return
    }
    Write-Host "-> Installing Tauri CLI (one-time)..."
    & $CargoExe install tauri-cli --version "^1" --locked
}

$CargoExe = Get-CargoExe
if (-not $CargoExe) {
    Write-Host ""
    Write-Host "Rust is not installed (cargo not found)." -ForegroundColor Red
    Write-Host ""
    Write-Host "Run this once from the gui folder:" -ForegroundColor Yellow
    Write-Host "  .\setup.ps1"
    Write-Host ""
    Write-Host "Or manually:" -ForegroundColor Yellow
    Write-Host "  winget install Rustlang.Rustup"
    Write-Host "  # restart terminal, then:"
    Write-Host "  cargo install tauri-cli --version `"^1`""
    Write-Host "  .\build.ps1"
    Write-Host ""
    exit 1
}

# Make sure ~/.cargo/bin is on PATH for this session
$cargoBin = Split-Path -Parent $CargoExe
if ($env:Path -notlike "*$cargoBin*") {
    $env:Path = "$cargoBin;$env:Path"
}

Ensure-TauriCli $CargoExe

$RuntimesDir = Join-Path $Here "resources\runtimes"
$PhpExe = Join-Path $RuntimesDir "php\php.exe"
if (-not (Test-Path $PhpExe)) {
    Write-Host ""
    Write-Host "Note: bundled PHP/.NET not installed yet." -ForegroundColor Yellow
    Write-Host "For PHP/ASP.NET local server, run once from repo root:" -ForegroundColor Yellow
    Write-Host "  .\scripts\setup-runtimes.ps1"
    Write-Host ""
}

$TargetDir = Join-Path $Here "src-tauri\target"
$env:CARGO_TARGET_DIR = $TargetDir

Push-Location (Join-Path $Here "src-tauri")
try {
    Write-Host "-> Building webcloner GUI..."
    & $CargoExe tauri build
    if ($LASTEXITCODE -ne 0) {
        exit $LASTEXITCODE
    }
}
finally {
    Pop-Location
}

Write-Host ""
Write-Host "Done. Find the GUI under src-tauri\target\release\bundle\" -ForegroundColor Green
