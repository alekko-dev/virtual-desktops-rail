function Assert-RailPackageVersion {
    param([string]$Requested, [string]$Actual)
    if ($Actual -notmatch '^\d+\.\d+\.\d+$') { throw 'Application version must be MAJOR.MINOR.PATCH.' }
    if ($Requested -and $Requested -ne $Actual) { throw "Package version '$Requested' does not match Cargo version '$Actual'." }
}

function Get-RailBuildInfo {
    param([string]$Executable)
    $command = [System.Diagnostics.Process]::new()
    $command.StartInfo.FileName = $Executable
    $command.StartInfo.Arguments = '--version --json'
    $command.StartInfo.UseShellExecute = $false
    $command.StartInfo.CreateNoWindow = $true
    $command.StartInfo.RedirectStandardOutput = $true
    $command.StartInfo.RedirectStandardError = $true
    try {
        if (!$command.Start()) { throw 'Cannot inspect executable build identity.' }
        $output = $command.StandardOutput.ReadToEndAsync()
        $errorOutput = $command.StandardError.ReadToEndAsync()
        if (!$command.WaitForExit(5000)) {
            $command.Kill()
            $command.WaitForExit()
            throw 'Build identity command timed out.'
        }
        if ($command.ExitCode) { throw "Build identity command failed: $($errorOutput.GetAwaiter().GetResult())" }
        return $output.GetAwaiter().GetResult() | ConvertFrom-Json
    } finally { $command.Dispose() }
}

function Assert-RailReleaseSource {
    param([string]$Root, [string]$Version, [string]$Tag)
    if ($Tag -cne "v$Version") { throw 'Release tag must exactly match vMAJOR.MINOR.PATCH from Cargo.' }
    $gitRoot = & git -C $Root rev-parse --show-toplevel
    if ($LASTEXITCODE -or !$gitRoot -or
        (Resolve-Path -LiteralPath $gitRoot).Path -ne (Resolve-Path -LiteralPath $Root).Path) {
        throw 'Release source must be the root of its own Git repository.'
    }
    $headCommit = & git -C $Root rev-parse --verify HEAD
    if ($LASTEXITCODE) { throw 'Release requires known Git provenance.' }
    $tagCommit = & git -C $Root rev-parse --verify "refs/tags/$Tag^{commit}"
    if ($LASTEXITCODE -or $tagCommit -cne $headCommit) { throw 'Release tag must point to HEAD.' }
    $changes = & git -C $Root status --porcelain --untracked-files=all
    if ($LASTEXITCODE -or $changes) { throw 'Release requires a clean working tree, including nonignored untracked files.' }
    return $headCommit
}

function Assert-RailReleaseBuild {
    param($Build, [string]$Commit)
    if (!$Build.commit -or $Build.commit -cne $Commit -or $Build.dirty -isnot [bool] -or $Build.dirty) {
        throw 'Release executable must have known, clean provenance matching the tagged source.'
    }
}
