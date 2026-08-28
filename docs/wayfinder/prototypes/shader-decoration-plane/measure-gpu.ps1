$ErrorActionPreference = 'Stop'

$source = @'
using System;
using System.Runtime.InteropServices;
public static class NativeGpuProbe {
  [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
  public static extern IntPtr CreateEvent(IntPtr attributes, bool manualReset, bool initialState, string name);
  [DllImport("kernel32.dll", SetLastError=true)]
  public static extern uint WaitForSingleObject(IntPtr handle, uint milliseconds);
  [DllImport("kernel32.dll")]
  public static extern bool CloseHandle(IntPtr handle);
}
'@

Add-Type -TypeDefinition $source
$results = @()

try {
    foreach ($name in @('dcomp-decoration-plane', 'gpui-decoration-plane')) {
        $readyName = "Local\KomorebiDecorationGpu-$([Guid]::NewGuid())"
        $readyEvent = [NativeGpuProbe]::CreateEvent([IntPtr]::Zero, $true, $false, $readyName)
        if ($readyEvent -eq [IntPtr]::Zero) {
            throw "CreateEvent failed for $name"
        }
        $env:KOMOREBI_READY_EVENT = $readyName
        $env:KOMOREBI_PROBE_REPORT = Join-Path $PWD "$name-gpu.json"
        $process = Start-Process -FilePath (Join-Path $PWD "target\release\$name.exe") -PassThru
        $readyWait = [NativeGpuProbe]::WaitForSingleObject($readyEvent, 5000)
        [NativeGpuProbe]::CloseHandle($readyEvent) | Out-Null
        if ($readyWait -ne 0) {
            throw "$name readiness event failed: $readyWait"
        }

        $samples = Get-Counter -Counter '\GPU Engine(*)\Utilization Percentage' -SampleInterval 1 -MaxSamples 4
        $matching = @($samples.CounterSamples | Where-Object InstanceName -Match "pid_$($process.Id)_")
        $byTimestamp = $matching | Group-Object Timestamp
        $totals = @($byTimestamp | ForEach-Object { ($_.Group.CookedValue | Measure-Object -Sum).Sum })
        $process.WaitForExit()
        $results += [pscustomobject]@{
            backend = $name
            process_id = $process.Id
            sample_count = $totals.Count
            mean_gpu_engine_percent = if ($totals.Count) { ($totals | Measure-Object -Average).Average } else { $null }
            peak_gpu_engine_percent = if ($totals.Count) { ($totals | Measure-Object -Maximum).Maximum } else { $null }
            note = 'Bounded four-second measurement sample; no production polling path.'
        }
    }
}
finally {
    Remove-Item Env:KOMOREBI_READY_EVENT, Env:KOMOREBI_PROBE_REPORT -ErrorAction SilentlyContinue
}

$results | ConvertTo-Json -Depth 3 | Set-Content -LiteralPath gpu-measurement.json -Encoding utf8
$results | ConvertTo-Json -Depth 3
