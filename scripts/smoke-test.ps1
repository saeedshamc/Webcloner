# Smoke test for webcloner CLI: clone + serve + HTTP 200 + stop
$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root

$env:CARGO_TARGET_DIR = Join-Path $Root "target"
Write-Host "Building..."
cargo build --quiet
$exe = Join-Path $Root "target\debug\webcloner.exe"
if (-not (Test-Path $exe)) { throw "binary missing: $exe" }

$out = Join-Path $Root "demo-output\smoke-plan"
if (Test-Path $out) { Remove-Item -Recurse -Force $out }

Write-Host "Cloning example.com..."
& $exe download "https://example.com" --out $out --max-pages 2 --max-depth 1 --concurrency 4
if ($LASTEXITCODE -ne 0) { throw "download failed" }
if (-not (Test-Path (Join-Path $out "index.html"))) { throw "index.html missing" }

$port = 8833
Write-Host "Serving on $port..."
$p = Start-Process -FilePath $exe -ArgumentList @("serve",$out,"--port",$port,"--auto-port") -PassThru -WindowStyle Hidden
Start-Sleep -Seconds 2
try {
  $r = Invoke-WebRequest -Uri "http://127.0.0.1:$port/" -UseBasicParsing -TimeoutSec 8
  if ($r.StatusCode -ne 200) { throw "bad status $($r.StatusCode)" }
  Write-Host "HTTP OK"
} finally {
  Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
  Get-Process webcloner -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
}

Write-Host "SMOKE PASSED"
