param(
    [ValidateRange(1,300)][int]$Seconds = 30,
    [ValidateRange(0,60)][int]$WarmupSeconds = 10,
    [ValidateRange(1,3)][int]$ExpectedManagedWindows = 2,
    [string]$OutputPath = 'artifacts/rail-populated-idle.json'
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repoRoot = Split-Path $PSScriptRoot -Parent
$control = Join-Path $PSScriptRoot 'Invoke-Rail.ps1'

function Read-RailStatus {
    $value = (& $control --status) | ConvertFrom-Json
    if (!$value.ready) { throw "Integration must be ready: $($value.status)" }
    if ($value.windows.Count -ne $ExpectedManagedWindows) { throw 'Unexpected managed-window count.' }
    if (@($value.windows | Where-Object { $_.pending -or $_.blocked -or !$_.responsive }).Count) {
        throw 'A managed window is pending, quarantined or unresponsive.'
    }
    return $value
}

function Read-Processes($status) {
    $main = Get-Process -Id $status.pid
    $children = @(Get-CimInstance Win32_Process -Filter "ParentProcessId = $($status.pid)" |
        Where-Object { $_.ExecutablePath -eq $main.Path -and $_.CommandLine -match '--integration-worker' })
    if ($children.Count -ne 1) { throw 'Expected one live integration worker.' }
    return @($main, (Get-Process -Id $children[0].ProcessId)) | ForEach-Object {
        [pscustomobject]@{ pid=$_.Id; created=$_.StartTime.ToUniversalTime().Ticks; executable=$_.Path;
            cpu_ms=$_.TotalProcessorTime.TotalMilliseconds; private_bytes=$_.PrivateMemorySize64; working_set_bytes=$_.WorkingSet64 }
    }
}

function Inventory-Signature($status) {
    return (@{ current=$status.current; generation=$status.generation; paused=$status.paused;
        windows=@($status.windows | Sort-Object key | Select-Object key,hwnd,pid,minimized,layout) } | ConvertTo-Json -Depth 12 -Compress)
}

function Process-Signature($processes) {
    return ($processes | Sort-Object pid | Select-Object pid,created,executable | ConvertTo-Json -Compress)
}

Push-Location $repoRoot
try {
    # Warm before taking counters. Do not drive UI, create/destroy windows, switch
    # desktops or clean fixtures until this command returns.
    $null = Read-RailStatus
    if ($WarmupSeconds) { Start-Sleep -Seconds $WarmupSeconds }
    $before = Read-RailStatus
    $beforeProcesses = @(Read-Processes $before)
    $inventory = Inventory-Signature $before
    $lifetimes = Process-Signature $beforeProcesses
    $watch = [Diagnostics.Stopwatch]::StartNew()
    $checks = 0
    do {
        Start-Sleep -Milliseconds ([int][Math]::Min(5000, [Math]::Max(1, $Seconds * 1000 - $watch.ElapsedMilliseconds)))
        $after = Read-RailStatus
        $afterProcesses = @(Read-Processes $after)
        if ((Inventory-Signature $after) -ne $inventory -or (Process-Signature $afterProcesses) -ne $lifetimes) {
            throw 'Desktop, managed inventory, layout, visibility or process lifetime changed; sample rejected.'
        }
        $checks++
    } while ($watch.Elapsed.TotalSeconds -lt $Seconds)
    $watch.Stop()
    $counters = @(Get-CimInstance Win32_PerfFormattedData_PerfProc_Process)
    $samples = @($afterProcesses | ForEach-Object {
        $process = $_
        $initial = $beforeProcesses | Where-Object pid -eq $process.pid
        $counter = @($counters | Where-Object IDProcess -eq $process.pid)
        if ($counter.Count -ne 1) { throw 'Private working-set counter missing or ambiguous.' }
        [pscustomobject]@{ pid=$process.pid; created_ticks=$process.created;
            cpu_ms=$process.cpu_ms-$initial.cpu_ms; private_bytes=$process.private_bytes;
            working_set_bytes=$process.working_set_bytes; private_working_set_bytes=[long]$counter[0].WorkingSetPrivate }
    })
    # Recheck lifetimes after counter collection as well as during the sample.
    if ((Process-Signature @(Read-Processes $after)) -ne $lifetimes) { throw 'Worker changed during counter collection.' }
    $report = @{ schema_version=1; measured_at_utc=[datetime]::UtcNow.ToString('o');
        duration_seconds=$watch.Elapsed.TotalSeconds; warmup_seconds=$WarmupSeconds;
        managed_windows=$ExpectedManagedWindows; stable_checks=$checks; processes=$samples;
        combined_private_working_set_bytes=($samples | Measure-Object private_working_set_bytes -Sum).Sum;
        combined_cpu_ms=($samples | Measure-Object cpu_ms -Sum).Sum;
        executable_sha256=(Get-FileHash -LiteralPath $beforeProcesses[0].executable -Algorithm SHA256).Hash;
        caveat='Finite CPU counter resolution; diagnostic polling overhead included. This does not measure action latency or certify physical hardware.' }
    $report | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $OutputPath -Encoding utf8
    $report | ConvertTo-Json -Depth 8
} finally { Pop-Location }
