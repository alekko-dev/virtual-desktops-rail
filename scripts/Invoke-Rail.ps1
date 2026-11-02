param([Parameter(ValueFromRemainingArguments = $true)][string[]]$RailArguments)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if (!$RailArguments -or $RailArguments[0] -notin @('--status','--pause','--resume','--retry','--exit','--help','--version','--click','--switch','--automation-ui','--experimental-integration','--auto-restore')) {
    throw 'Use this helper for a rail control command; launch the GUI directly with Start-Process.'
}
$taskExecutable = Join-Path (Split-Path $PSScriptRoot -Parent) 'target/release/virtual-desktop-rail.exe'
$taskProcess = [System.Diagnostics.Process]::new()
$taskProcess.StartInfo.FileName = $taskExecutable
$taskProcess.StartInfo.UseShellExecute = $false
$taskProcess.StartInfo.CreateNoWindow = $true
$taskProcess.StartInfo.RedirectStandardOutput = $true
$taskProcess.StartInfo.RedirectStandardError = $true
foreach ($taskArgument in $RailArguments) { $taskProcess.StartInfo.ArgumentList.Add($taskArgument) }
try {
    if (!$taskProcess.Start()) { throw 'Could not start the rail control command.' }
    $taskOutput = $taskProcess.StandardOutput.ReadToEndAsync()
    $taskError = $taskProcess.StandardError.ReadToEndAsync()
    if (!$taskProcess.WaitForExit(5000)) {
        $taskProcess.Kill()
        $taskProcess.WaitForExit()
        throw 'Rail control command timed out; only its owned command process was stopped.'
    }
    $taskText = $taskOutput.GetAwaiter().GetResult()
    $taskErrorText = $taskError.GetAwaiter().GetResult()
    if ($taskProcess.ExitCode -ne 0) { throw "Rail control command failed ($($taskProcess.ExitCode)): $taskErrorText" }
    if ($taskText) { Write-Output $taskText.TrimEnd() }
} finally { $taskProcess.Dispose() }
