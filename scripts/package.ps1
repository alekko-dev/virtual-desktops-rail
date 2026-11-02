$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repoRoot = Split-Path $PSScriptRoot -Parent
$cargo = Join-Path $env:USERPROFILE '.cargo/bin/cargo.exe'
$rustc = Join-Path $env:USERPROFILE '.cargo/bin/rustc.exe'
Push-Location $repoRoot
try {
    & $cargo build -p virtual-desktop-rail --release --locked
    if ($LASTEXITCODE) { throw 'Release build failed.' }
    $metadataText = & $cargo metadata --format-version 1 --filter-platform x86_64-pc-windows-msvc --locked --offline
    if ($LASTEXITCODE) { throw 'Dependency metadata failed.' }
    $metadata = $metadataText | ConvertFrom-Json
    $name = 'VirtualDesktopRail-0.1.0-windows-x64-' + (Get-Date -Format 'yyyyMMdd-HHmmss')
    $package = Join-Path $repoRoot "artifacts/$name"
    if (Test-Path -LiteralPath $package) { throw 'Package destination already exists.' }
    New-Item -ItemType Directory -Path "$package/licenses" -Force | Out-Null
    Copy-Item -LiteralPath 'target/release/virtual-desktop-rail.exe' -Destination $package
    Copy-Item -LiteralPath '.devdocs/Portable-Build.md' -Destination "$package/README.md"
    $notices = @('Third-party dependencies from Cargo.lock. See accompanying license files.', '')
    foreach ($dependency in $metadata.packages | Where-Object source | Sort-Object name,version) {
        $notices += "$($dependency.name) $($dependency.version): $($dependency.license)"
        $source = Split-Path $dependency.manifest_path -Parent
        $destination = Join-Path "$package/licenses" "$($dependency.name)-$($dependency.version)"
        New-Item -ItemType Directory -Path $destination | Out-Null
        $licenseFiles = @(Get-ChildItem -LiteralPath $source -File | Where-Object { $_.Name -match '^(LICENSE|LICENCE|COPYING|COPYRIGHT)' })
        if (!$licenseFiles.Count) { throw "Missing license files for $($dependency.name)" }
        foreach ($license in $licenseFiles) { Copy-Item -LiteralPath $license.FullName -Destination $destination }
    }
    $rustRoot = & $rustc --print sysroot
    if ($LASTEXITCODE) { throw 'Rust sysroot lookup failed.' }
    Copy-Item -LiteralPath "$rustRoot/share/doc/rust/licenses" -Destination "$package/licenses/rust-standard-library" -Recurse
    Copy-Item -LiteralPath "$rustRoot/share/doc/rust/COPYRIGHT-library.html" -Destination "$package/licenses/rust-standard-library"
    $notices | Set-Content -LiteralPath "$package/THIRD-PARTY-NOTICES.txt" -Encoding utf8
    Get-FileHash -LiteralPath "$package/virtual-desktop-rail.exe" -Algorithm SHA256 |
        Select-Object Algorithm,Hash | ConvertTo-Json | Set-Content -LiteralPath "$package/SHA256.json" -Encoding utf8
    Compress-Archive -LiteralPath $package -DestinationPath "$package.zip"
    Write-Output "$package.zip"
} finally { Pop-Location }
