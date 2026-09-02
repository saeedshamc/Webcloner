# Usage: .\setup.ps1
#
# One-time setup: installs Rust (via winget) and Tauri CLI for building the GUI.

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

$CargoExe = Get-CargoExe
if (-not $CargoExe) {
    Write-Host "-> Installing Rust via winget..."
    winget install Rustlang.Rustup --accept-package-agreements --accept-source-agreements
    $CargoExe = Join-Path $env:USERPROFILE ".cargo\bin\cargo.exe"
    if (-not (Test-Path $CargoExe)) {
        Write-Error "Rust install finished but cargo.exe was not found. Restart the terminal and run .\setup.ps1 again."
    }
    $env:Path = "$(Split-Path -Parent $CargoExe);$env:Path"
}

Write-Host "-> Rust: $(& $CargoExe --version)"
Write-Host "-> Installing Tauri CLI..."
& $CargoExe install tauri-cli --version "^1" --locked

Write-Host ""
Write-Host "Setup complete. Now run:" -ForegroundColor Green
Write-Host "  .\build.ps1"
