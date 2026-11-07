$ErrorActionPreference = 'Stop'
$projectDir = Split-Path -Parent $PSScriptRoot
$engineManifest = Join-Path $projectDir 'third_party/cold-clear-2/Cargo.toml'
cargo build --release --locked --manifest-path $engineManifest --bin cold-clear-2
if ($LASTEXITCODE -ne 0) { throw 'Cold Clear 2 build failed' }
$engineOutput = Join-Path $projectDir 'engines'
New-Item -ItemType Directory -Force $engineOutput | Out-Null
Copy-Item -LiteralPath (Join-Path $projectDir 'third_party/cold-clear-2/target/release/cold-clear-2.exe') -Destination (Join-Path $engineOutput 'cold-clear-2.exe') -Force
Write-Output 'Cold Clear 2 executable built and installed.'
