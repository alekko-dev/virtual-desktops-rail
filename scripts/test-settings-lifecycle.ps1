param([string]$Executable = "$PSScriptRoot/../target/release/settings/rail-settings.exe")
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
# Isolated pipe/lifecycle smoke. Does not connect to the resident rail or change preferences.
# A Settings window may appear briefly; this is not pointer/keyboard/rendering validation.
$process = [Diagnostics.Process]::new()
$process.StartInfo.FileName = (Resolve-Path -LiteralPath $Executable).Path
$process.StartInfo.ArgumentList.Add('--pipe')
$process.StartInfo.UseShellExecute = $false
$process.StartInfo.CreateNoWindow = $true
$process.StartInfo.RedirectStandardInput = $true
$process.StartInfo.RedirectStandardOutput = $true
$process.StartInfo.RedirectStandardError = $true
$state = [ordered]@{
    version = 1; acknowledged = 0; loaded = $true; paused = $false; startup = $false
    startup_busy = $false; report_busy = $false; auto_restore = $false
    minimize_snapped_elsewhere = $true; desktop_tint = $false; show_desktop_name = $true
    light_switch_target = 'applications'; status = 'Isolated lifecycle check'; notice = ''; build = 'test'
}
try {
    if (!$process.Start()) { throw 'Could not start the owned Settings fixture.' }
    $errors = $process.StandardError.ReadToEndAsync()
    $process.StandardInput.WriteLine(($state | ConvertTo-Json -Compress))
    $ready = $process.StandardOutput.ReadLineAsync()
    if (!$ready.Wait(15000)) { throw 'Settings did not acknowledge initialization within 15 seconds.' }
    if (!$ready.Result) { throw "Settings exited before initialization: $($errors.GetAwaiter().GetResult())" }
    $reply = $ready.Result | ConvertFrom-Json
    if ($reply.version -ne 1 -or $reply.id -ne 0 -or $reply.action.action -ne 'ready') { throw 'Invalid initialization acknowledgement.' }
    $state.paused = $true
    $process.StandardInput.WriteLine(($state | ConvertTo-Json -Compress))
    $process.StandardInput.Close()
    if (!$process.WaitForExit(10000)) { throw 'Settings did not exit after the parent pipe closed.' }
    if ($process.ExitCode -ne 0) { throw "Settings failed ($($process.ExitCode)): $($errors.GetAwaiter().GetResult())" }
    Write-Output 'Settings initialized WinUI, accepted a state update, and exited on pipe closure. Rendering and real input remain unverified.'
} finally {
    if ($process.Id -and !$process.HasExited) { $process.Kill(); $process.WaitForExit() }
    $process.Dispose()
}
