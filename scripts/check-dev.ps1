$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repoRoot = Split-Path $PSScriptRoot -Parent
$cargo = Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe'
if (!(Test-Path $cargo)) { throw 'Install Rust using rustup first.' }
Push-Location $repoRoot
try {
    $manifest = Join-Path $repoRoot 'tools\native-smoke\Cargo.toml'
    & $cargo fmt --manifest-path $manifest --check
    if ($LASTEXITCODE) { throw 'Formatting check failed.' }
    & $cargo clippy --manifest-path $manifest --all-targets --locked -- -D warnings
    if ($LASTEXITCODE) { throw 'Clippy failed.' }
    & $cargo test --manifest-path $manifest --locked
    if ($LASTEXITCODE) { throw 'Tests failed.' }
    & $cargo build --manifest-path $manifest --release --locked
    if ($LASTEXITCODE) { throw 'Release build failed.' }
    Write-Host 'Native toolchain checks passed. Interactive validation is separate.'
} finally { Pop-Location }
