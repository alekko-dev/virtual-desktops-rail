$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repoRoot = Split-Path $PSScriptRoot -Parent
$cargo = Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe'
if (!(Test-Path $cargo)) { throw 'Install Rust using rustup first.' }
Push-Location $repoRoot
try {
    & $cargo fmt --all --check
    if ($LASTEXITCODE) { throw 'Formatting check failed.' }
    & $cargo clippy --workspace --all-targets --locked -- -D warnings
    if ($LASTEXITCODE) { throw 'Clippy failed.' }
    & $cargo test --workspace --locked
    if ($LASTEXITCODE) { throw 'Tests failed.' }
    & "$PSScriptRoot/test-versioning.ps1"
    & $cargo build --workspace --release --locked
    if ($LASTEXITCODE) { throw 'Release build failed.' }
    $workspace = & $cargo metadata --no-deps --format-version 1 --locked | ConvertFrom-Json
    if ($LASTEXITCODE) { throw 'Workspace metadata failed.' }
    & "$PSScriptRoot/Stage-Settings.ps1" -BuildDirectory (Join-Path $workspace.target_directory 'release')
    Write-Host 'Workspace checks passed. Interactive compatibility validation is separate.'
} finally { Pop-Location }
