param([Parameter(Mandatory)][string]$BuildDirectory)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
# Allowlist from windows-reactor-setup 0.100.0 assets/runtime.txt (MIT/Apache-2.0).
# Copy only the helper and its runtime; never copy arbitrary build-output files.
$build = (Resolve-Path -LiteralPath $BuildDirectory).Path
$destination = Join-Path $build 'settings'
New-Item -ItemType Directory -Path $destination -Force | Out-Null
$files = @('rail-settings.exe', 'rail.ico', 'Microsoft.Web.WebView2.Core.dll') + @(Get-Content "$PSScriptRoot/settings-runtime-files.txt" | Where-Object { $_.Trim() })
foreach ($file in $files) {
    $source = Join-Path $build $file
    if (Test-Path -LiteralPath $source) { Copy-Item -LiteralPath $source -Destination $destination -Recurse -Force }
}
foreach ($required in @('rail-settings.exe', 'rail.ico', 'Microsoft.UI.Xaml.dll', 'Microsoft.WindowsAppRuntime.dll', 'resources.pri')) {
    if (!(Test-Path -LiteralPath (Join-Path $destination $required) -PathType Leaf)) {
        throw "Settings runtime staging is incomplete: $required"
    }
}
Add-Type -AssemblyName System.IO.Compression.FileSystem
$runtimeCache = Join-Path $env:LOCALAPPDATA 'windows-reactor-setup/temp'
foreach ($runtimePackage in @('Microsoft.WindowsAppSDK.Runtime.2.4.0', 'Microsoft.Web.WebView2.1.0.4078.44')) {
    $archivePath = Join-Path $runtimeCache "$runtimePackage.nupkg"
    if (!(Test-Path -LiteralPath $archivePath -PathType Leaf)) { throw "Missing runtime license source: $archivePath" }
    $licenseDirectory = Join-Path $destination "licenses/$runtimePackage"
    New-Item -ItemType Directory -Path $licenseDirectory -Force | Out-Null
    $archive = [IO.Compression.ZipFile]::OpenRead($archivePath)
    try {
        foreach ($name in @('LICENSE.txt', 'NOTICE.txt')) {
            $entry = $archive.Entries | Where-Object { $_.FullName -ieq $name } | Select-Object -First 1
            if (!$entry) { throw "Missing $name in $runtimePackage" }
            [IO.Compression.ZipFileExtensions]::ExtractToFile($entry, (Join-Path $licenseDirectory $name), $true)
        }
    } finally { $archive.Dispose() }
}
Write-Output $destination
