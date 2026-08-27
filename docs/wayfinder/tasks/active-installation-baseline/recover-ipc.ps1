$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$client = 'C:\Program Files\komorebi\bin\komorebic.exe'
$dataDirectory = 'C:\Users\jonfo\AppData\Local\komorebi'
$runtimeTarget = 'C:\Users\jonfo\.local\share\komorebi\runtime'
$startupDirectory = [Environment]::GetFolderPath('Startup')
$managerStartup = Join-Path $startupDirectory 'Komorebi.lnk'
$barStartup = Join-Path $startupDirectory 'Komorebi AppBar.lnk'
$expectedBar = 'C:\Users\jonfo\AppData\Local\Programs\komorebi-personal\installations\bar-0.1.41-1687650786b1\komorebi-bar.exe'

$manager = @(Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'komorebi.exe' })
if ($manager.Count -gt 1) {
    throw "Expected at most one manager process, found $($manager.Count)"
}
$managerProcessId = if ($manager.Count -eq 1) { [int]$manager[0].ProcessId } else { $null }

$consoleSignalSource = @'
using System;
using System.Runtime.InteropServices;

public static class ConsoleSignal
{
    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool AttachConsole(uint processId);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool FreeConsole();

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool GenerateConsoleCtrlEvent(uint eventType, uint processGroupId);

    [DllImport("kernel32.dll")]
    private static extern bool SetConsoleCtrlHandler(IntPtr handler, bool add);

    public static bool TryCtrlC(uint processId)
    {
        SetConsoleCtrlHandler(IntPtr.Zero, true);
        try
        {
            if (!AttachConsole(processId))
            {
                return false;
            }
            try
            {
                return GenerateConsoleCtrlEvent(0, 0);
            }
            finally
            {
                FreeConsole();
            }
        }
        finally
        {
            SetConsoleCtrlHandler(IntPtr.Zero, false);
        }
    }
}
'@

Add-Type -TypeDefinition $consoleSignalSource
$signalSent = $false
$shutdownMode = 'already-stopped'
if ($null -ne $managerProcessId) {
    $restoreOutput = & $client restore-windows 2>&1
    if ($LASTEXITCODE -ne 0) {
        throw "restore-windows failed: $($restoreOutput -join ' ')"
    }

    $signalSent = [ConsoleSignal]::TryCtrlC([uint32]$managerProcessId)
    $deadline = (Get-Date).AddSeconds(6)
    do {
        Start-Sleep -Milliseconds 100
        $managerAlive = $null -ne (Get-Process -Id $managerProcessId -ErrorAction SilentlyContinue)
    } while ($managerAlive -and (Get-Date) -lt $deadline)

    $shutdownMode = 'ctrl-c'
    if ($managerAlive) {
        Stop-Process -Id $managerProcessId -ErrorAction Stop
        Wait-Process -Id $managerProcessId -Timeout 5 -ErrorAction SilentlyContinue
        $shutdownMode = 'restore-windows-plus-stop-process'
    }
}

Get-Process -Name 'komorebi-bar' -ErrorAction SilentlyContinue | Stop-Process -Force
$deadline = (Get-Date).AddSeconds(5)
do {
    Start-Sleep -Milliseconds 100
    $barAlive = @(Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'komorebi-bar.exe' }).Count -gt 0
} while ($barAlive -and (Get-Date) -lt $deadline)
if ($barAlive) {
    throw 'AppBar did not stop with its scheduled task'
}

$nativeDeleteSource = @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;

public static class NativeFileDelete
{
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern bool DeleteFile(string path);

    public static void Delete(string path)
    {
        if (!DeleteFile(path))
        {
            throw new Win32Exception(Marshal.GetLastWin32Error(), "DeleteFileW failed for " + path);
        }
    }
}
'@
Add-Type -TypeDefinition $nativeDeleteSource

$markers = @(Get-ChildItem -LiteralPath $dataDirectory -Force | Where-Object {
    $_.Name -eq 'komorebi.sock' -or $_.Name -like 'komorebi-bar-*'
})
$cleanupMode = 'individual-delete'
$quarantineDirectories = [Collections.Generic.List[string]]::new()
try {
    foreach ($marker in $markers) {
        $resolved = [IO.Path]::GetFullPath($marker.FullName)
        if (-not $resolved.StartsWith($dataDirectory + '\', [StringComparison]::OrdinalIgnoreCase)) {
            throw "Refusing to remove marker outside the komorebi data directory: $resolved"
        }
        [NativeFileDelete]::Delete($resolved)
    }
} catch {
    $dataPath = [IO.Path]::GetFullPath($dataDirectory).TrimEnd('\')
    $parentPath = [IO.Path]::GetFullPath((Split-Path -Parent $dataDirectory)).TrimEnd('\')
    if ((Split-Path -Parent $dataPath) -ne $parentPath) {
        throw "Refusing to quarantine an unexpected data directory: $dataPath"
    }
    $attempt = 0
    while (Test-Path -LiteralPath $dataPath) {
        if ($attempt -ge 8) {
            throw "The direct runtime directory remained after $attempt quarantine attempts"
        }
        $quarantineDirectory = $dataPath + '.stale-' + (Get-Date -Format 'yyyyMMddTHHmmss') + '-' + $attempt
        Move-Item -LiteralPath $dataPath -Destination $quarantineDirectory
        $quarantineDirectories.Add($quarantineDirectory)
        $attempt++
    }
    New-Item -ItemType Directory -Path $runtimeTarget -Force | Out-Null
    $targetMarkers = @(Get-ChildItem -LiteralPath $runtimeTarget -Force -ErrorAction SilentlyContinue | Where-Object {
        $_.Name -eq 'komorebi.sock' -or $_.Name -like 'komorebi-bar-*'
    })
    if ($targetMarkers.Count -gt 0) {
        throw "The verified runtime target unexpectedly contains $($targetMarkers.Count) socket markers"
    }
    New-Item -ItemType Junction -Path $dataPath -Target $runtimeTarget | Out-Null
    $cleanupMode = 'junction-to-profile-runtime'
}

if (-not (Test-Path -LiteralPath $managerStartup)) {
    throw "manager startup shortcut is missing: $managerStartup"
}
Start-Process -FilePath $managerStartup

$deadline = (Get-Date).AddSeconds(10)
do {
    Start-Sleep -Milliseconds 200
    $queryOutput = & $client state 2>&1
    $queryExit = $LASTEXITCODE
} while ($queryExit -ne 0 -and (Get-Date) -lt $deadline)
if ($queryExit -ne 0) {
    throw "state query remained unavailable: $($queryOutput -join ' ')"
}
$null = $queryOutput -join "`n" | ConvertFrom-Json

if (-not (Test-Path -LiteralPath $barStartup)) {
    throw "AppBar startup shortcut is missing: $barStartup"
}
Start-Process -FilePath $barStartup
$deadline = (Get-Date).AddSeconds(8)
do {
    Start-Sleep -Milliseconds 100
    $bar = @(Get-CimInstance Win32_Process | Where-Object {
        $_.Name -eq 'komorebi-bar.exe' -and $_.ExecutablePath -eq $expectedBar
    })
} while ($bar.Count -ne 1 -and (Get-Date) -lt $deadline)
if ($bar.Count -ne 1) {
    throw 'AppBar did not restart from the immutable path'
}

Start-Sleep -Milliseconds 500
$markersAfter = @(Get-ChildItem -LiteralPath $dataDirectory -Force | Where-Object {
    $_.Name -eq 'komorebi.sock' -or $_.Name -like 'komorebi-bar-*'
} | Select-Object Name, FullName, CreationTime)

[pscustomobject]@{
    manager_process_id_before = $managerProcessId
    ctrl_c_signal_sent = $signalSent
    shutdown_mode = $shutdownMode
    cleanup_mode = $cleanupMode
    quarantine_directories = $quarantineDirectories
    runtime_target = $runtimeTarget
    removed_markers = @($markers | ForEach-Object { $_.Name })
    manager_process_id_after = @(Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'komorebi.exe' } | ForEach-Object { $_.ProcessId })
    bar_process_id_after = @($bar | ForEach-Object { $_.ProcessId })
    query_bytes = [Text.Encoding]::UTF8.GetByteCount(($queryOutput -join "`n"))
    markers_after = @($markersAfter | ForEach-Object { $_.Name })
} | ConvertTo-Json -Depth 5
