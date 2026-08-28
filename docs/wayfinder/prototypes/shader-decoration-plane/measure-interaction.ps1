$ErrorActionPreference = 'Stop'

$source = @'
using System;
using System.Runtime.InteropServices;
public static class NativeInput {
  delegate bool EnumWindowsCallback(IntPtr window, IntPtr parameter);
  [StructLayout(LayoutKind.Sequential)] public struct Point { public int X; public int Y; }
  [StructLayout(LayoutKind.Sequential)] public struct Rect { public int Left; public int Top; public int Right; public int Bottom; }
  [StructLayout(LayoutKind.Sequential)] public struct MouseInput { public int dx; public int dy; public uint mouseData; public uint flags; public uint time; public UIntPtr extraInfo; }
  [StructLayout(LayoutKind.Explicit)] public struct InputUnion { [FieldOffset(0)] public MouseInput mi; }
  [StructLayout(LayoutKind.Sequential)] public struct Input { public uint type; public InputUnion u; }
  [DllImport("user32.dll")] public static extern bool GetCursorPos(out Point point);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr window, out Rect rect);
  [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(Point point);
  [DllImport("user32.dll")] static extern bool EnumWindows(EnumWindowsCallback callback, IntPtr parameter);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);
  [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr window);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] static extern bool SetWindowPos(IntPtr window, IntPtr insertAfter, int x, int y, int width, int height, uint flags);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr window);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)] public static extern IntPtr CreateEvent(IntPtr attributes, bool manualReset, bool initialState, string name);
  [DllImport("kernel32.dll", SetLastError=true)] public static extern uint WaitForSingleObject(IntPtr handle, uint milliseconds);
  [DllImport("kernel32.dll")] public static extern bool CloseHandle(IntPtr handle);
  [DllImport("user32.dll", SetLastError=true)] static extern uint SendInput(uint count, Input[] inputs, int size);
  public static IntPtr FindWindow(uint wantedProcessId) {
    IntPtr found = IntPtr.Zero;
    EnumWindows((window, parameter) => {
      uint processId;
      GetWindowThreadProcessId(window, out processId);
      if (processId == wantedProcessId && IsWindowVisible(window)) { found = window; return false; }
      return true;
    }, IntPtr.Zero);
    return found;
  }
  public static bool RaiseWithoutActivation(IntPtr window) {
    return SetWindowPos(window, new IntPtr(-1), 0, 0, 0, 0, 0x0001 | 0x0002 | 0x0010);
  }
  public static uint Click(int x, int y) {
    SetCursorPos(x, y);
    var inputs = new Input[] {
      new Input { type = 0, u = new InputUnion { mi = new MouseInput { flags = 0x0002 } } },
      new Input { type = 0, u = new InputUnion { mi = new MouseInput { flags = 0x0004 } } }
    };
    return SendInput(2, inputs, Marshal.SizeOf(typeof(Input)));
  }
}
'@

Add-Type -TypeDefinition $source
[NativeInput+Point]$original = New-Object 'NativeInput+Point'
[NativeInput]::GetCursorPos([ref]$original) | Out-Null
$results = @()

try {
    foreach ($overlayName in @('none', 'dcomp-decoration-plane', 'gpui-decoration-plane')) {
        $clickReport = Join-Path $PWD "$overlayName-click.json"
        $overlayReport = Join-Path $PWD "$overlayName-interaction.json"
        Remove-Item -LiteralPath $clickReport -ErrorAction SilentlyContinue

        $env:KOMOREBI_CLICK_REPORT = $clickReport
        $targetReadyName = "Local\KomorebiDecorationTarget-$([Guid]::NewGuid())"
        $targetReadyEvent = [NativeInput]::CreateEvent([IntPtr]::Zero, $true, $false, $targetReadyName)
        if ($targetReadyEvent -eq [IntPtr]::Zero) {
            throw "target CreateEvent failed"
        }
        $env:KOMOREBI_READY_EVENT = $targetReadyName
        $target = Start-Process -FilePath .\target\release\decoration-interaction-target.exe -PassThru
        $targetReadyWait = [NativeInput]::WaitForSingleObject($targetReadyEvent, 5000)
        [NativeInput]::CloseHandle($targetReadyEvent) | Out-Null
        if ($targetReadyWait -ne 0) {
            throw "target readiness event failed: $targetReadyWait"
        }
        $targetWindow = [NativeInput]::FindWindow($target.Id)
        [NativeInput+Rect]$targetRect = New-Object 'NativeInput+Rect'
        if ($targetWindow -eq [IntPtr]::Zero -or -not [NativeInput]::GetWindowRect($targetWindow, [ref]$targetRect)) {
            throw "GetWindowRect failed for interaction target"
        }

        $overlay = $null
        $overlayWindow = [IntPtr]::Zero
        if ($overlayName -ne 'none') {
            $env:KOMOREBI_PROBE_REPORT = $overlayReport
            $readyName = "Local\KomorebiDecorationProbe-$([Guid]::NewGuid())"
            $readyEvent = [NativeInput]::CreateEvent([IntPtr]::Zero, $true, $false, $readyName)
            if ($readyEvent -eq [IntPtr]::Zero) {
                throw "CreateEvent failed"
            }
            $env:KOMOREBI_READY_EVENT = $readyName
            $overlay = Start-Process -FilePath (Join-Path $PWD "target\release\$overlayName.exe") -PassThru
            $readyWait = [NativeInput]::WaitForSingleObject($readyEvent, 5000)
            [NativeInput]::CloseHandle($readyEvent) | Out-Null
            if ($readyWait -ne 0) {
                throw "overlay readiness event failed: $readyWait"
            }
            $overlayWindow = [NativeInput]::FindWindow($overlay.Id)
            if ($overlayWindow -eq [IntPtr]::Zero) {
                throw "overlay did not create a top-level window"
            }
        }
        $targetRaised = [NativeInput]::RaiseWithoutActivation($targetWindow)
        $foregroundSet = [NativeInput]::SetForegroundWindow($targetWindow)
        if ($overlayWindow -ne [IntPtr]::Zero) {
            $overlayRaised = [NativeInput]::RaiseWithoutActivation($overlayWindow)
        } else {
            $overlayRaised = $null
        }
        $x = [int](($targetRect.Left + $targetRect.Right) / 2)
        $y = [int](($targetRect.Top + $targetRect.Bottom) / 2)
        $point = [NativeInput+Point]::new()
        $point.X = $x
        $point.Y = $y
        $windowAtPoint = [NativeInput]::WindowFromPoint($point)
        [uint32]$processAtPoint = 0
        $null = [NativeInput]::GetWindowThreadProcessId($windowAtPoint, [ref]$processAtPoint)
        $sent = [NativeInput]::Click($x, $y)
        $received = $target.WaitForExit(2000)
        if (-not $received) {
            $target.Kill()
            $target.WaitForExit()
        }
        if ($null -ne $overlay) {
            $overlay.WaitForExit()
        }
        $results += [pscustomobject]@{
            overlay = $overlayName
            inputs_sent = $sent
            target_process_id = $target.Id
            overlay_process_id = if ($null -ne $overlay) { $overlay.Id } else { $null }
            process_at_click_point = $processAtPoint
            target_raised = $targetRaised
            overlay_raised = $overlayRaised
            foreground_set = $foregroundSet
            foreground_is_target = [NativeInput]::GetForegroundWindow() -eq $targetWindow
            target_received_click = $received -and (Test-Path -LiteralPath $clickReport)
            report = if (Test-Path -LiteralPath $clickReport) {
                Get-Content -LiteralPath $clickReport -Raw | ConvertFrom-Json
            } else {
                $null
            }
        }
    }
}
finally {
    [NativeInput]::SetCursorPos($original.X, $original.Y) | Out-Null
    Remove-Item Env:KOMOREBI_CLICK_REPORT, Env:KOMOREBI_PROBE_REPORT, Env:KOMOREBI_READY_EVENT -ErrorAction SilentlyContinue
}

$results | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath interaction-measurements.json -Encoding utf8
$results | ConvertTo-Json -Depth 4
