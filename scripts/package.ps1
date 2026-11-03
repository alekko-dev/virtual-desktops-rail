param(
    [string]$Version,
    [string]$BuildLabel,
    [string]$ReleaseTag
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repoRoot = Split-Path $PSScriptRoot -Parent
$cargo = Join-Path $env:USERPROFILE '.cargo/bin/cargo.exe'
$rustc = Join-Path $env:USERPROFILE '.cargo/bin/rustc.exe'
Push-Location $repoRoot
try {
    . "$PSScriptRoot/version-tools.ps1"
    $applicationManifest = & $cargo metadata --no-deps --format-version 1 --locked
    if ($LASTEXITCODE) { throw 'Application metadata lookup failed.' }
    $applicationMetadata = $applicationManifest | ConvertFrom-Json
    $application = $applicationMetadata.packages |
        Where-Object { $_.name -eq 'virtual-desktop-rail' } | Select-Object -First 1
    if (!$application) { throw 'Application package metadata was not found.' }
    Assert-RailPackageVersion -Requested $Version -Actual $application.version
    $Version = $application.version
    $releaseCommit = $null
    if ($ReleaseTag) { $releaseCommit = Assert-RailReleaseSource -Root $repoRoot -Version $Version -Tag $ReleaseTag }
    if ($BuildLabel -and $BuildLabel -notmatch '^[a-zA-Z0-9][a-zA-Z0-9.-]*$') { throw 'Build label must contain only letters, digits, dots and hyphens.' }

    & $cargo build -p virtual-desktop-rail --release --locked
    if ($LASTEXITCODE) { throw 'Release build failed.' }
    $executable = Join-Path $applicationMetadata.target_directory 'release/virtual-desktop-rail.exe'
    $build = Get-RailBuildInfo -Executable $executable
    $fileVersion = [System.Diagnostics.FileVersionInfo]::GetVersionInfo($executable)
    $productVersion = "$Version+unknown"
    if ($build.commit -and $build.dirty -is [bool]) {
        $productVersion = "$Version+g$($build.commit.Substring(0, 12))"
        if ($build.dirty) { $productVersion += '.dirty' }
    }
    if ($build.version -ne $Version -or $fileVersion.FileVersion -ne "$Version.0" -or
        $fileVersion.ProductVersion -ne $productVersion -or
        $fileVersion.ProductMajorPart -ne $fileVersion.FileMajorPart -or
        $fileVersion.ProductMinorPart -ne $fileVersion.FileMinorPart -or
        $fileVersion.ProductBuildPart -ne $fileVersion.FileBuildPart -or $fileVersion.ProductPrivatePart -ne 0) {
        throw 'Executable version resources do not match Cargo and embedded build identity.'
    }
    if ($ReleaseTag) {
        $verifiedCommit = Assert-RailReleaseSource -Root $repoRoot -Version $Version -Tag $ReleaseTag
        if ($verifiedCommit -cne $releaseCommit) { throw 'Release source changed during build.' }
        Assert-RailReleaseBuild -Build $build -Commit $releaseCommit
    }
    $metadataText = & $cargo metadata --format-version 1 --filter-platform x86_64-pc-windows-msvc --locked --offline
    if ($LASTEXITCODE) { throw 'Dependency metadata failed.' }
    $metadata = $metadataText | ConvertFrom-Json
    $name = "VirtualDesktopRail-$Version-windows-x64"
    if ($BuildLabel) { $name = "VirtualDesktopRail-$Version-$BuildLabel-windows-x64" }
    $package = Join-Path $repoRoot "artifacts/$name"
    if (Test-Path -LiteralPath $package) { throw 'Package destination already exists.' }
    New-Item -ItemType Directory -Path "$package/licenses" -Force | Out-Null
    Copy-Item -LiteralPath $executable -Destination $package
    $readme = Get-Content -LiteralPath 'README.md' -Raw -Encoding utf8
    $repositorySection = '(?s)\r?\n<!-- repository-links:start -->.*?<!-- repository-links:end -->\r?\n?'
    if ([regex]::Matches($readme, $repositorySection).Count -ne 1) {
        throw 'README must contain exactly one marked repository-only section.'
    }
    $packageReadme = [regex]::Replace($readme, $repositorySection, [Environment]::NewLine)
    Set-Content -LiteralPath "$package/README.md" -Value $packageReadme.TrimEnd() -Encoding utf8
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
    [ordered]@{
        version = $build.version
        commit = $build.commit
        dirty = $build.dirty
        build_label = $(if ($BuildLabel) { $BuildLabel } else { $null })
        sha256 = (Get-FileHash -LiteralPath "$package/virtual-desktop-rail.exe" -Algorithm SHA256).Hash
    } | ConvertTo-Json | Set-Content -LiteralPath "$package/BUILD.json" -Encoding utf8
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    [System.IO.Compression.ZipFile]::CreateFromDirectory(
        $package,
        "$package.zip",
        [System.IO.Compression.CompressionLevel]::Optimal,
        $true
    )
    Write-Output "$package.zip"
} finally { Pop-Location }
