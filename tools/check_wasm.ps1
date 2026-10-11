# Builds the simulation stack for wasm32-unknown-unknown and the headless `web` entry crate.
#
# Usage (from the repository root):
#   powershell -ExecutionPolicy Bypass -File tools/check_wasm.ps1
#
# Exit code 0 when every build passes. Set $env:CARGO_JOBS to change the parallelism (default 2).
# Optional: when wasm-bindgen-cli is on PATH, JS glue is written to target/web-js (not committed).

$ErrorActionPreference = "Continue"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$jobs = if ($env:CARGO_JOBS) { $env:CARGO_JOBS } else { "2" }
$target = "wasm32-unknown-unknown"
$crates = @("mpq", "w3g", "jass", "softfloat", "sim", "gamedata", "w3map", "blp", "mdx", "fdf", "engine")

$installed = rustup target list --installed
if ($installed -notcontains $target) {
    rustup target add $target
    if ($LASTEXITCODE -ne 0) { throw "rustup target add $target failed" }
}

$failed = @()
foreach ($c in $crates) {
    cargo build -j $jobs -p $c --target $target *> $null
    if ($LASTEXITCODE -eq 0) { Write-Host "ok    $c" } else { Write-Host "FAIL  $c"; $failed += $c }
}

cargo build -j $jobs -p web --target $target --release *> $null
if ($LASTEXITCODE -eq 0) {
    $wasm = Join-Path $root "target/$target/release/web.wasm"
    $size = (Get-Item $wasm).Length
    Write-Host ("ok    web  ({0:N0} bytes)" -f $size)
    if (Get-Command wasm-bindgen -ErrorAction SilentlyContinue) {
        $out = Join-Path $root "target/web-js"
        wasm-bindgen --target web --out-dir $out $wasm
        if ($LASTEXITCODE -eq 0) { Write-Host "ok    JS glue in $out" } else { $failed += "web-bindgen" }
    } else {
        Write-Host "skip  JS glue (wasm-bindgen-cli not on PATH)"
    }
} else {
    Write-Host "FAIL  web"; $failed += "web"
}

if ($failed.Count -gt 0) {
    Write-Host ("failed: " + ($failed -join ", "))
    exit 1
}
Write-Host "all wasm32 builds passed"
exit 0
