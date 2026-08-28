param(
    [string]$ResultsDirectory = (Join-Path $PSScriptRoot 'results')
)

$ErrorActionPreference = 'Stop'

function Get-Percentile {
    param(
        [double[]]$Values,
        [double]$Percentile
    )

    if ($Values.Count -eq 0) {
        return $null
    }

    $sorted = @($Values | Sort-Object)
    $rank = ($sorted.Count - 1) * $Percentile
    $lower = [math]::Floor($rank)
    $upper = [math]::Ceiling($rank)
    if ($lower -eq $upper) {
        return [math]::Round($sorted[$lower], 1)
    }

    $weight = $rank - $lower
    return [math]::Round(
        $sorted[$lower] + (($sorted[$upper] - $sorted[$lower]) * $weight),
        1
    )
}

function Get-ApiOutcomes {
    param($Run)

    $counts = [ordered]@{
        desktop_id_ok = 0L
        desktop_id_error = 0L
        membership_ok = 0L
        membership_error = 0L
        cloak_ok = 0L
        cloak_error = 0L
    }
    $hresults = @{}

    foreach ($aggregate in $Run.window_aggregates.PSObject.Properties) {
        foreach ($outcomeProperty in $aggregate.Value.outcomes.PSObject.Properties) {
            $weight = [long]$outcomeProperty.Value
            $observation = $outcomeProperty.Name | ConvertFrom-Json
            foreach ($field in @('desktop_id', 'on_current_desktop', 'cloaked')) {
                $value = $observation.$field
                $prefix = switch ($field) {
                    'desktop_id' { 'desktop_id' }
                    'on_current_desktop' { 'membership' }
                    'cloaked' { 'cloak' }
                }
                if ($value.status -eq 'ok') {
                    $counts["${prefix}_ok"] += $weight
                }
                else {
                    $counts["${prefix}_error"] += $weight
                    $key = "$field/$($value.hresult)"
                    if (-not $hresults.ContainsKey($key)) {
                        $hresults[$key] = 0L
                    }
                    $hresults[$key] += $weight
                }
            }
        }
    }

    [pscustomobject]@{
        counts = [pscustomobject]$counts
        hresults = [pscustomobject]$hresults
    }
}

$pollingFiles = Get-ChildItem -LiteralPath $ResultsDirectory -Filter '*restart-*.json' |
    Where-Object Name -Match '^(pre|post)-restart-(16|100|500)ms\.json$' |
    Sort-Object Name
if ($pollingFiles.Count -ne 6) {
    throw "expected six pre/post polling captures, found $($pollingFiles.Count)"
}

$runs = foreach ($file in $pollingFiles) {
    $run = Get-Content -LiteralPath $file.FullName -Raw | ConvertFrom-Json
    $latencies = @($run.transitions | ForEach-Object { [double]$_.input_to_stable_ms })
    $api = Get-ApiOutcomes $run
    $cpuMs = [long]$run.user_cpu_ms + [long]$run.kernel_cpu_ms
    [pscustomobject]@{
        phase = $run.phase
        interval_ms = [int]$run.interval_ms
        completed = [int]$run.completed_transitions
        timed_out = [bool]$run.timed_out
        latency_min_ms = if ($latencies.Count) { ($latencies | Measure-Object -Minimum).Minimum } else { $null }
        latency_p50_ms = Get-Percentile $latencies 0.50
        latency_p95_ms = Get-Percentile $latencies 0.95
        latency_max_ms = if ($latencies.Count) { ($latencies | Measure-Object -Maximum).Maximum } else { $null }
        max_signature_changes = if ($run.transitions.Count) { ($run.transitions.signature_changes_before_stable | Measure-Object -Maximum).Maximum } else { $null }
        polls = [long]$run.poll_count
        public_queries = [long]$run.public_query_count
        elapsed_ms = [long]$run.elapsed_ms
        query_rate_per_second = [math]::Round(([long]$run.public_query_count * 1000.0) / [long]$run.elapsed_ms, 1)
        process_cpu_ms = $cpuMs
        process_cpu_percent = [math]::Round(($cpuMs * 100.0) / [long]$run.elapsed_ms, 2)
        api = $api
    }
}

$native = Get-Content -LiteralPath (Join-Path $ResultsDirectory 'native-events.json') -Raw | ConvertFrom-Json
$nativeSummary = [pscustomobject]@{
    duration_ms = [long]$native.duration_ms
    total_events = @($native.events).Count
    desktop_window_name_events = @($native.events | Where-Object {
        $_.kind -eq 'object_name_changed' -and $_.window_alias -eq 'desktop_window'
    }).Count
    system_desktop_switch_events = @($native.events | Where-Object kind -eq 'system_desktop_switch').Count
    normal_window_cloak_events = @($native.events | Where-Object {
        $_.window_alias -eq 'w01' -and $_.kind -in @('object_cloaked', 'object_uncloaked')
    }).Count
    pinned_window_cloak_events = @($native.events | Where-Object {
        $_.window_alias -eq 'w02' -and $_.kind -in @('object_cloaked', 'object_uncloaked')
    }).Count
    process_cpu_ms = [long]$native.user_cpu_ms + [long]$native.kernel_cpu_ms
}

$incompleteRuns = @($runs | Where-Object { $_.completed -ne 10 -or $_.timed_out })
if ($incompleteRuns.Count -ne 0) {
    throw "one or more polling captures did not complete ten transitions"
}
if ($nativeSummary.desktop_window_name_events -ne $nativeSummary.normal_window_cloak_events) {
    throw "desktop-window and normal-window wake counts diverged"
}
if ($nativeSummary.desktop_window_name_events -eq 0) {
    throw "native capture contains no desktop-window wake"
}
if ($nativeSummary.system_desktop_switch_events -ne 0) {
    throw "EVENT_SYSTEM_DESKTOPSWITCH unexpectedly fired during Task View switching"
}
if ($nativeSummary.pinned_window_cloak_events -ne 0) {
    throw "pinned probe unexpectedly emitted a cloak transition"
}

$summary = [pscustomobject]@{
    generated_from = @($pollingFiles.Name) + 'native-events.json'
    polling_runs = @($runs)
    native_events = $nativeSummary
}

$summaryPath = Join-Path $ResultsDirectory 'summary.json'
$summary | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $summaryPath -Encoding utf8

$lines = @(
    '# Generated result summary',
    '',
    '| Phase | Interval | Complete | Min | P50 | P95 | Max | Queries/s | CPU | API errors |',
    '| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |'
)
foreach ($run in $runs) {
    $errors = $run.api.counts.desktop_id_error + $run.api.counts.membership_error + $run.api.counts.cloak_error
    $lines += "| $($run.phase) | $($run.interval_ms) ms | $($run.completed)/10 | $($run.latency_min_ms) ms | $($run.latency_p50_ms) ms | $($run.latency_p95_ms) ms | $($run.latency_max_ms) ms | $($run.query_rate_per_second) | $($run.process_cpu_percent)% | $errors |"
}
$lines += @(
    '',
    '## Native event capture',
    '',
    "- Desktop-window name events: $($nativeSummary.desktop_window_name_events)",
    "- `EVENT_SYSTEM_DESKTOPSWITCH` events: $($nativeSummary.system_desktop_switch_events)",
    "- Normal-window cloak/uncloak events: $($nativeSummary.normal_window_cloak_events)",
    "- Pinned-window cloak/uncloak events: $($nativeSummary.pinned_window_cloak_events)",
    "- Process CPU: $($nativeSummary.process_cpu_ms) ms over $($nativeSummary.duration_ms) ms"
)
$lines | Set-Content -LiteralPath (Join-Path $ResultsDirectory 'summary.md') -Encoding utf8

$summary
