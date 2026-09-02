# Usage: .\package.ps1 C:\path\to\cloned-site
#
# Copies a folder produced by `webcloner download` into dist/, then builds a
# native, fully offline desktop executable with Tauri.

param(
    [Parameter(Mandatory = $true, Position = 0)]
    [string]$SourceDir
)

$ErrorActionPreference = "Stop"

$Here = Split-Path -Parent $MyInvocation.MyCommand.Path
$Dist = Join-Path $Here "dist"

if (-not (Test-Path $SourceDir -PathType Container)) {
    Write-Error "Error: '$SourceDir' is not a directory (point this at the folder webcloner downloaded)"
}

Write-Host "-> Clearing $Dist"
if (Test-Path $Dist) {
    Remove-Item -Recurse -Force $Dist
}
New-Item -ItemType Directory -Path $Dist | Out-Null

Write-Host "-> Copying $SourceDir into $Dist"
Copy-Item -Path (Join-Path $SourceDir "*") -Destination $Dist -Recurse -Force

$IndexPath = Join-Path $Dist "index.html"
if (-not (Test-Path $IndexPath)) {
    Write-Warning "Warning: no index.html found at the root of $SourceDir - the window may open blank."
}

Write-Host "-> Building the desktop app (requires Tauri CLI: cargo install tauri-cli --version ^1)"
Push-Location (Join-Path $Here "src-tauri")
try {
    cargo tauri build
}
finally {
    Pop-Location
}

Write-Host "Done. Find your installer/executable under src-tauri/target/release/bundle/"
