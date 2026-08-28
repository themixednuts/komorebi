$ErrorActionPreference = 'Stop'

$source = @'
using System;
using System.Runtime.InteropServices;
public static class NativeLifecycle {
  [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
  public static extern IntPtr CreateEvent(IntPtr attributes, bool manualReset, bool initialState, string name);
  [DllImport("kernel32.dll", SetLastError=true)]
  public static extern uint WaitForSingleObject(IntPtr handle, uint milliseconds);
  [DllImport("kernel32.dll")]
  public static extern bool CloseHandle(IntPtr handle);
}
'@

Add-Type -TypeDefinition $source
$probes = @()

try {
    foreach ($name in @('dcomp-decoration-plane', 'gpui-decoration-plane')) {
        $readyName = "Local\KomorebiDecorationLifecycle-$([Guid]::NewGuid())"
        $readyEvent = [NativeLifecycle]::CreateEvent([IntPtr]::Zero, $true, $false, $readyName)
        if ($readyEvent -eq [IntPtr]::Zero) {
            throw "CreateEvent failed for $name"
        }
        $env:KOMOREBI_READY_EVENT = $readyName
        $env:KOMOREBI_PROBE_REPORT = Join-Path $PWD "$name-explorer-restart.json"
        $process = Start-Process -FilePath (Join-Path $PWD "target\release\$name.exe") -PassThru
        $readyWait = [NativeLifecycle]::WaitForSingleObject($readyEvent, 5000)
        [NativeLifecycle]::CloseHandle($readyEvent) | Out-Null
        if ($readyWait -ne 0) {
            throw "$name readiness event failed: $readyWait"
        }
        $probes += [pscustomobject]@{ name = $name; process = $process }
    }

    $oldExplorer = @(Get-Process explorer -ErrorAction SilentlyContinue)
    $oldExplorer | Stop-Process -Force
    $newExplorer = Start-Process -FilePath explorer.exe -PassThru
    $inputIdle = $newExplorer.WaitForInputIdle(5000)

    $results = foreach ($probe in $probes) {
        [pscustomobject]@{
            probe = $probe.name
            process_id = $probe.process.Id
            ready_before_restart = $true
            alive_after_explorer_input_idle = -not $probe.process.HasExited
            completed_normally = $probe.process.WaitForExit(10000) -and $probe.process.ExitCode -eq 0
        }
    }

    [pscustomobject]@{
        old_explorer_process_count = $oldExplorer.Count
        new_explorer_process_id = $newExplorer.Id
        new_explorer_reached_input_idle = $inputIdle
        probes = @($results)
    } | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath explorer-restart-measurement.json -Encoding utf8
}
finally {
    Remove-Item Env:KOMOREBI_READY_EVENT, Env:KOMOREBI_PROBE_REPORT -ErrorAction SilentlyContinue
    foreach ($probe in $probes) {
        if (-not $probe.process.HasExited) {
            $probe.process.Kill()
            $probe.process.WaitForExit()
        }
    }
}

Get-Content -Raw -LiteralPath explorer-restart-measurement.json
