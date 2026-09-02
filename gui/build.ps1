# Usage: .\build.ps1
#
# Builds the webcloner graphical interface (Tauri app).

$ErrorActionPreference = "Stop"
$Here = Split-Path -Parent $MyInvocation.MyCommand.Path
Push-Location (Join-Path $Here "src-tauri")
try {
    cargo tauri build
}
finally {
    Pop-Location
}

Write-Host "Done. Find the GUI under src-tauri/target/release/bundle/"
