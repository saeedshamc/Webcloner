# Usage: .\run.ps1 download https://example.com --out my-site --zip
#
# Runs the pre-built webcloner binary via WSL (Linux build from Docker).
# Requires WSL with Ubuntu and the binary at target/release/webcloner.

param(
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$CliArgs
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $MyInvocation.MyCommand.Path
$BinaryWin = Join-Path $Root "target\release\webcloner"

if (-not (Test-Path $BinaryWin)) {
    Write-Error "Binary not found at $BinaryWin. Build first:`n  docker run --rm -v `"${Root}:/app`" -w /app rust:1-bookworm /usr/local/cargo/bin/cargo build --release"
}

function Convert-ToWslPath([string]$Path) {
    $full = [System.IO.Path]::GetFullPath($Path)
    if ($full -match '^([A-Za-z]):\\(.*)$') {
        $drive = $Matches[1].ToLower()
        $rest = $Matches[2] -replace '\\', '/'
        return "/mnt/$drive/$rest"
    }
    return ($full -replace '\\', '/')
}

$WslRoot = Convert-ToWslPath $Root
$WslBinary = Convert-ToWslPath $BinaryWin
if ($CliArgs.Count -eq 0) {
    $CliArgs = @("--help")
}

$argLine = ($CliArgs | ForEach-Object {
    $value = $_ -replace '\\', '/'
    if ($value -match '\s') { "'$value'" } else { $value }
}) -join ' '

wsl -d Ubuntu-26.04 -- bash -lc "cd '$WslRoot' && '$WslBinary' $argLine"
exit $LASTEXITCODE
