# Downloads portable PHP and .NET SDK into gui/resources/runtimes/
# so the GUI can serve PHP / ASP.NET without relying on system PATH.
# Run once from repo root:  .\scripts\setup-runtimes.ps1

$ErrorActionPreference = "Stop"

$Root = Split-Path -Parent $PSScriptRoot
$RuntimesDir = Join-Path $Root "gui\resources\runtimes"
$PhpDir = Join-Path $RuntimesDir "php"
$DotnetDir = Join-Path $RuntimesDir "dotnet"

New-Item -ItemType Directory -Force -Path $RuntimesDir | Out-Null

function Expand-Zip($ZipPath, $DestDir) {
    if (Test-Path $DestDir) {
        Remove-Item -Recurse -Force $DestDir
    }
    New-Item -ItemType Directory -Force -Path $DestDir | Out-Null
    Expand-Archive -Path $ZipPath -DestinationPath $DestDir -Force
}

# --- PHP (Windows x64 NTS) ---
$PhpExe = Join-Path $PhpDir "php.exe"
if (-not (Test-Path $PhpExe)) {
    Write-Host "Downloading PHP..."
    $phpIndex = Invoke-RestMethod "https://windows.php.net/downloads/releases/releases.json"
    $phpVersion = ($phpIndex | Get-Member -MemberType NoteProperty | Select-Object -First 1).Name
    $phpMeta = $phpIndex.$phpVersion
    $zipName = ($phpMeta.nts.x64 | Where-Object { $_ -match "vs16" } | Select-Object -First 1)
    if (-not $zipName) {
        $zipName = $phpMeta.nts.x64[0]
    }
    $phpZipUrl = "https://windows.php.net/downloads/releases/$zipName"
    $phpZip = Join-Path $env:TEMP "webcloner-php.zip"
    Invoke-WebRequest -Uri $phpZipUrl -OutFile $phpZip -UseBasicParsing
    $extractTemp = Join-Path $env:TEMP "webcloner-php-extract"
    Expand-Zip $phpZip $extractTemp
    $inner = Get-ChildItem $extractTemp -Directory | Select-Object -First 1
    if ($inner) {
        Move-Item $inner.FullName $PhpDir
    } else {
        Move-Item $extractTemp $PhpDir
    }
    Remove-Item $phpZip -Force -ErrorAction SilentlyContinue
    Write-Host "PHP installed: $PhpExe"
} else {
    Write-Host "PHP already present: $PhpExe"
}

# --- .NET SDK (portable, for dotnet run) ---
$DotnetExe = Join-Path $DotnetDir "dotnet.exe"
if (-not (Test-Path $DotnetExe)) {
    Write-Host "Downloading .NET SDK (portable)..."
    $installScript = Join-Path $env:TEMP "dotnet-install.ps1"
    Invoke-WebRequest -Uri "https://dot.net/v1/dotnet-install.ps1" -OutFile $installScript -UseBasicParsing
    & $installScript -InstallDir $DotnetDir -Channel 8.0 -Quality ga
    if (-not (Test-Path $DotnetExe)) {
        throw ".NET install failed — dotnet.exe not found in $DotnetDir"
    }
    Write-Host ".NET installed: $DotnetExe"
} else {
    Write-Host ".NET already present: $DotnetExe"
}

Write-Host ""
Write-Host "Done. Runtimes are in: $RuntimesDir"
Write-Host "Rebuild the GUI so Tauri bundles them:  cd gui; .\build.ps1"
