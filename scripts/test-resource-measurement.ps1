# Run directly with pwsh -NoProfile -File scripts/test-resource-measurement.ps1.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$tokens = $null
$parseErrors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot 'Measure-RailIdle.ps1'), [ref]$tokens, [ref]$parseErrors)
if ($parseErrors.Count) { throw ($parseErrors | Out-String) }
foreach ($name in @('Summarize-Memory','Compare-Reports','Inventory-Signature')) {
    $definition = $ast.Find({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name}.GetNewClosure(), $true)
    Invoke-Expression $definition.Extent.Text
}
function Assert($condition, $message) { if (!$condition) { throw $message } }
$samples = @(10,30,20 | ForEach-Object { [pscustomobject]@{private_bytes=$_; private_working_set_bytes=$_*2; working_set_bytes=$_*3} })
$memory = Summarize-Memory $samples
Assert ($memory.private_bytes.mean -eq 20 -and $memory.private_bytes.sampled_peak -eq 30 -and $memory.private_bytes.growth -eq 10) 'Memory aggregation failed.'
$context = [ordered]@{machine='test';os_version='test';logical_processors=4;workload='two windows';managed_windows=2;requested_seconds=30;warmup_seconds=10;sample_interval_seconds=5;paused=$false;desktop_count=2}
$old = [pscustomobject]@{schema_version=2;context=[pscustomobject]$context;memory=$memory;cpu_ms_per_second=0;cpu_percent_one_core=0;executable_bytes=100}
$new = $old | ConvertTo-Json -Depth 12 | ConvertFrom-Json
$new.executable_bytes=120
$new.cpu_ms_per_second=1
$new.memory.private_bytes.mean=25
$rows = @(Compare-Reports $old $new)
$size = $rows | Where-Object metric -eq executable_bytes
Assert ($size.delta -eq 20 -and $size.percent -eq 20) 'Size delta failed.'
$mean = $rows | Where-Object metric -eq private_bytes.mean
Assert ($mean.delta -eq 5 -and $mean.percent -eq 25) 'Memory delta failed.'
Assert ($null -eq ($rows | Where-Object metric -eq cpu_ms_per_second).percent) 'Zero baseline must not divide by zero.'
foreach ($field in $context.Keys) {
    $copy = $new | ConvertTo-Json -Depth 12 | ConvertFrom-Json
    $copy.context.$field = 'different'
    $rejected = $false
    try { $null = Compare-Reports $old $copy } catch { $rejected = $true }
    Assert $rejected "Context mismatch accepted: $field"
}
$old.schema_version=1
$rejected=$false
try { $null=Compare-Reports $old $new } catch { $rejected=$true }
Assert $rejected 'Legacy schema accepted.'
$status=[pscustomobject]@{current='a';generation=1;paused=$false;windows=@()}
Assert ((Inventory-Signature $status) -eq (Inventory-Signature $status)) 'Inventory signature must be deterministic.'
Write-Host 'Resource aggregation, deltas, zero baseline, schema and context guards passed.'
