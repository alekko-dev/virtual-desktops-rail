param([switch]$Provenance)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
. "$PSScriptRoot/version-tools.ps1"
function Expect-Failure {
    param([scriptblock]$Action)
    $failed = $false
    try { & $Action } catch { $failed = $true }
    if (!$failed) { throw 'Expected version validation to reject this input.' }
}
Assert-RailPackageVersion -Requested '0.1.0' -Actual '0.1.0'
Assert-RailPackageVersion -Actual '0.1.0'
Expect-Failure { Assert-RailPackageVersion -Requested '9.0.0' -Actual '0.1.0' }
Assert-RailReleaseBuild -Build ([pscustomobject]@{commit='abc';dirty=$false}) -Commit 'abc'
foreach ($build in @(
    [pscustomobject]@{commit='abc';dirty=$true},
    [pscustomobject]@{commit=$null;dirty=$null},
    [pscustomobject]@{commit='def';dirty=$false}
)) { Expect-Failure { Assert-RailReleaseBuild -Build $build -Commit 'abc' } }

if (!$Provenance) { Write-Output 'Version contract checks passed.'; return }
$repoRoot = Split-Path $PSScriptRoot -Parent
$fixture = Join-Path $repoRoot "artifacts/version-fixture-$([guid]::NewGuid().ToString('N'))"
New-Item -ItemType Directory -Path $fixture | Out-Null
# Include current modifications and new source files, never the user's Git directory.
$files = & git -C $repoRoot ls-files --cached --others --exclude-standard
if ($LASTEXITCODE) { throw 'Cannot inventory source for isolated fixture.' }
foreach ($file in $files) {
    $source = Join-Path $repoRoot $file
    if (!(Test-Path -LiteralPath $source -PathType Leaf)) { continue }
    $destination = Join-Path $fixture $file
    New-Item -ItemType Directory -Path (Split-Path $destination -Parent) -Force | Out-Null
    Copy-Item -LiteralPath $source -Destination $destination
}
$cargo = Join-Path $env:USERPROFILE '.cargo/bin/cargo.exe'
$previousTarget = $env:CARGO_TARGET_DIR
$env:CARGO_TARGET_DIR = Join-Path $fixture 'target'
function Build-Fixture {
    & $cargo build --manifest-path "$fixture/Cargo.toml" -p virtual-desktop-rail --locked --offline
    if ($LASTEXITCODE) { throw 'Fixture build failed.' }
    return Get-RailBuildInfo -Executable "$fixture/target/debug/virtual-desktop-rail.exe"
}
function Git-Fixture {
    param([Parameter(ValueFromRemainingArguments=$true)][string[]]$Arguments)
    $result = & git -C $fixture @Arguments
    if ($LASTEXITCODE) { throw "Fixture Git command failed: $Arguments" }
    return $result
}
try {
    $unknown = Build-Fixture
    if ($null -ne $unknown.commit -or $null -ne $unknown.dirty) { throw 'Exported source inherited enclosing provenance.' }
    Expect-Failure { Assert-RailReleaseSource -Root $fixture -Version '0.1.0' -Tag 'v0.1.0' }
    Git-Fixture init | Out-Null
    Git-Fixture add .
    Git-Fixture -c user.name=VersionFixture -c user.email=fixture@example.invalid commit -m 'Version fixture' | Out-Null
    $clean = Build-Fixture
    if (!$clean.commit -or $clean.dirty -ne $false) { throw 'Clean source identity failed.' }
    Git-Fixture tag v0.1.0
    $commit = Assert-RailReleaseSource -Root $fixture -Version '0.1.0' -Tag 'v0.1.0'
    Assert-RailReleaseBuild -Build $clean -Commit $commit
    Expect-Failure { Assert-RailReleaseSource -Root $fixture -Version '0.1.0' -Tag 'v9.0.0' }
    Add-Content -LiteralPath "$fixture/README.md" -Value 'Fixture tracked change'
    $dirty = Build-Fixture
    if ($dirty.commit -ne $clean.commit -or $dirty.dirty -ne $true) { throw 'Tracked change did not refresh identity.' }
    Expect-Failure { Assert-RailReleaseSource -Root $fixture -Version '0.1.0' -Tag 'v0.1.0' }
    Git-Fixture restore README.md
    Set-Content -LiteralPath "$fixture/untracked-fixture.txt" -Value 'Fixture untracked change'
    $dirty = Build-Fixture
    if ($dirty.dirty -ne $true) { throw 'Untracked change did not refresh identity.' }
    Expect-Failure { Assert-RailReleaseSource -Root $fixture -Version '0.1.0' -Tag 'v0.1.0' }
    Git-Fixture add untracked-fixture.txt
    Git-Fixture -c user.name=VersionFixture -c user.email=fixture@example.invalid commit -m 'New fixture commit' | Out-Null
    $changed = Build-Fixture
    if ($changed.commit -eq $clean.commit -or $changed.dirty -ne $false) { throw 'Commit-only change did not refresh identity.' }
    Expect-Failure { Assert-RailReleaseSource -Root $fixture -Version '0.1.0' -Tag 'v0.1.0' }
    $previousPath = $env:PATH
    try {
        $env:PATH = (($previousPath -split ';') | Where-Object { $_ -and !(Test-Path -LiteralPath (Join-Path $_ 'git.exe')) }) -join ';'
        if (Get-Command git -ErrorAction SilentlyContinue) { throw 'Git is still discoverable in unavailable-Git test.' }
        $withoutGit = Build-Fixture
        if ($null -ne $withoutGit.commit -or $null -ne $withoutGit.dirty) { throw 'Unavailable Git must produce unknown provenance.' }
        Expect-Failure { Assert-RailReleaseBuild -Build $withoutGit -Commit $changed.commit }
    } finally { $env:PATH = $previousPath }
    Write-Output "Isolated provenance and release checks passed: $fixture"
} finally { $env:CARGO_TARGET_DIR = $previousTarget }
