param(
    [ValidateRange(1,300)][int]$Seconds = 30,
    [ValidateRange(0,60)][int]$WarmupSeconds = 10,
    [ValidateRange(0,100)][int]$ExpectedManagedWindows = 2,
    [ValidateRange(1,30)][int]$SampleIntervalSeconds = 5,
    [Parameter(Mandatory)][ValidateNotNullOrEmpty()][string]$Workload,
    [string]$Label = '',
    [string]$BaselinePath,
    [string]$OutputPath
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
    return ([ordered]@{ current=$status.current; generation=$status.generation; paused=$status.paused;
        windows=@($status.windows | Sort-Object key | Select-Object key,hwnd,pid,minimized,layout) } | ConvertTo-Json -Depth 12 -Compress)
}

function Process-Signature($processes) {
    return ($processes | Sort-Object pid | Select-Object pid,created,executable | ConvertTo-Json -Compress)
}

function Read-MemorySample($processes, [double]$elapsed) {
    $counters = @(Get-CimInstance Win32_PerfFormattedData_PerfProc_Process)
    $values = @($processes | ForEach-Object {
        $process = $_
        $counter = @($counters | Where-Object IDProcess -eq $process.pid)
        if ($counter.Count -ne 1) { throw 'Private working-set counter missing or ambiguous.' }
        [pscustomobject]@{ pid=$process.pid; private_bytes=$process.private_bytes;
            working_set_bytes=$process.working_set_bytes; private_working_set_bytes=[long]$counter[0].WorkingSetPrivate }
    })
    return [pscustomobject]@{ elapsed_seconds=$elapsed; processes=$values;
        private_working_set_bytes=($values | Measure-Object private_working_set_bytes -Sum).Sum;
        private_bytes=($values | Measure-Object private_bytes -Sum).Sum;
        working_set_bytes=($values | Measure-Object working_set_bytes -Sum).Sum }
}

function Summarize-Memory($samples) {
    $summary = [ordered]@{}
    foreach ($metric in @('private_working_set_bytes','private_bytes','working_set_bytes')) {
        $stats = $samples | Measure-Object -Property $metric -Average -Maximum
        $summary[$metric] = [ordered]@{ start=$samples[0].$metric; end=$samples[-1].$metric;
            mean=$stats.Average; sampled_peak=$stats.Maximum; growth=$samples[-1].$metric-$samples[0].$metric }
    }
    return $summary
}

function Compare-Reports($baseline, $current) {
    if ($baseline.schema_version -ne 2) { throw 'Baseline must be a schema-2 resource report.' }
    foreach ($field in @('machine','os_version','logical_processors','workload','managed_windows','requested_seconds','warmup_seconds','sample_interval_seconds','paused','desktop_count')) {
        if ($baseline.context.$field -cne $current.context.$field) {
            throw "Baseline context differs: $field. Use the same machine, workload and measurement settings."
        }
    }
    $rows = @()
    foreach ($metric in @('private_working_set_bytes','private_bytes','working_set_bytes')) {
        foreach ($stat in @('mean','sampled_peak','end','growth')) {
            $old = [double]$baseline.memory.$metric.$stat
            $new = [double]$current.memory.$metric.$stat
            $rows += [pscustomobject]@{ metric="$metric.$stat"; baseline=$old; current=$new;
                delta=$new-$old; percent= $(if ($old -gt 0) { 100*($new-$old)/$old } else { $null }) }
        }
    }
    foreach ($metric in @('cpu_ms_per_second','cpu_percent_one_core','executable_bytes')) {
        $old = [double]$baseline.$metric
        $new = [double]$current.$metric
        $rows += [pscustomobject]@{ metric=$metric; baseline=$old; current=$new;
            delta=$new-$old; percent= $(if ($old -gt 0) { 100*($new-$old)/$old } else { $null }) }
    }
    return $rows
}

Push-Location $repoRoot
try {
    $baseline = if ($BaselinePath) { Get-Content -LiteralPath $BaselinePath -Raw | ConvertFrom-Json } else { $null }
    if (!$OutputPath) { $OutputPath = 'artifacts/resource-runs/{0}-{1}.json' -f [datetime]::UtcNow.ToString('yyyyMMddTHHmmssfffZ'),[guid]::NewGuid().ToString('N').Substring(0,8) }
    if (Test-Path -LiteralPath $OutputPath) { throw 'Output already exists; choose a new path to preserve history.' }
    $os = Get-CimInstance Win32_OperatingSystem
    $context = [ordered]@{ machine=$env:COMPUTERNAME; os_version=$os.Version;
        logical_processors=[Environment]::ProcessorCount; workload=$Workload; managed_windows=$ExpectedManagedWindows;
        requested_seconds=$Seconds; warmup_seconds=$WarmupSeconds; sample_interval_seconds=$SampleIntervalSeconds }
    # Leave desktops, windows and settings untouched throughout this idle measurement.
    $null = Read-RailStatus
    if ($WarmupSeconds) { Start-Sleep -Seconds $WarmupSeconds }
    $before = Read-RailStatus
    $context.paused = $before.paused
    $context.desktop_count = @($before.desktops).Count
    $beforeProcesses = @(Read-Processes $before)
    $watch = [Diagnostics.Stopwatch]::StartNew()
    $inventory = Inventory-Signature $before
    $lifetimes = Process-Signature $beforeProcesses
    $executable = $beforeProcesses[0].executable
    $hash = (Get-FileHash -LiteralPath $executable -Algorithm SHA256).Hash
    $size = (Get-Item -LiteralPath $executable).Length
    $samples = [Collections.Generic.List[object]]::new()
    $samples.Add((Read-MemorySample $beforeProcesses $watch.Elapsed.TotalSeconds))
    do {
        Start-Sleep -Milliseconds ([int][Math]::Min($SampleIntervalSeconds*1000, [Math]::Max(1, $Seconds*1000-$watch.ElapsedMilliseconds)))
        $after = Read-RailStatus
        $afterProcesses = @(Read-Processes $after)
        if ((Inventory-Signature $after) -ne $inventory -or (Process-Signature $afterProcesses) -ne $lifetimes) {
            throw 'Desktop, managed inventory, layout, visibility or process lifetime changed; sample rejected.'
        }
        $cpuElapsed = $watch.Elapsed.TotalSeconds
        $samples.Add((Read-MemorySample $afterProcesses $cpuElapsed))
    } while ($watch.Elapsed.TotalSeconds -lt $Seconds)
    $watch.Stop()
    $final = Read-RailStatus
    if ((Inventory-Signature $final) -ne $inventory -or (Process-Signature @(Read-Processes $final)) -ne $lifetimes) {
        throw 'Workload or process changed during counter collection.'
    }
    if ((Get-FileHash -LiteralPath $executable -Algorithm SHA256).Hash -ne $hash) { throw 'Executable changed during measurement.' }
    $cpuMs = ($afterProcesses | Measure-Object cpu_ms -Sum).Sum - ($beforeProcesses | Measure-Object cpu_ms -Sum).Sum
    $report = [ordered]@{ schema_version=2; measured_at_utc=[datetime]::UtcNow.ToString('o'); label=$Label;
        context=$context; duration_seconds=$cpuElapsed; executable_sha256=$hash; executable_bytes=$size;
        processes=$beforeProcesses; samples=@($samples.ToArray()); memory=(Summarize-Memory $samples.ToArray());
        combined_cpu_ms=$cpuMs; cpu_ms_per_second=$cpuMs/$cpuElapsed; cpu_percent_one_core=($cpuMs/$cpuElapsed/10);
        comparison=$null;
        caveat='Idle only; finite CPU resolution and diagnostic polling overhead. Peaks are sampled; means are unweighted sample means. Total working set can double-count shared pages. Matching context cannot ensure equal host load or application state. Not action latency or leak certification.' }
    if ($baseline) {
        $report.comparison = [ordered]@{ baseline_path=[IO.Path]::GetFullPath($BaselinePath);
            baseline_sha256=$baseline.executable_sha256; deltas=@(Compare-Reports $baseline ([pscustomobject]$report)) }
    }
    $parent = Split-Path ([IO.Path]::GetFullPath($OutputPath)) -Parent
    $null = New-Item -ItemType Directory -Path $parent -Force
    $json = $report | ConvertTo-Json -Depth 12
    # CreateNew prevents accidental baseline/history replacement, including races.
    $stream = [IO.File]::Open([IO.Path]::GetFullPath($OutputPath), [IO.FileMode]::CreateNew)
    try {
        $writer = [IO.StreamWriter]::new($stream)
        try { $writer.WriteLine($json) } finally { $writer.Dispose() }
    } finally { $stream.Dispose() }
    Write-Host "Saved: $OutputPath"
    Write-Host ('Combined mean memory: private resident {0:N2} MiB; private committed {1:N2} MiB; total resident {2:N2} MiB (shared pages may repeat).' -f ($report.memory.private_working_set_bytes.mean/1MB),($report.memory.private_bytes.mean/1MB),($report.memory.working_set_bytes.mean/1MB))
    Write-Host ('CPU: {0:N3}% of one core; executable: {1:N2} MiB.' -f $report.cpu_percent_one_core,($size/1MB))
    if ($baseline) {
        $report.comparison.deltas | Format-Table metric,baseline,current,delta,@{Label='percent';Expression={if ($null -eq $_.percent) {'n/a'} else {'{0:+0.00;-0.00;0}%' -f $_.percent}}} -AutoSize | Out-Host
    }
    Write-Output $json
} finally { Pop-Location }
