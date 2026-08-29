# Measured GPU compute and WARP fallback

## Result

All three admitted paths opened on this machine and stayed inside the 240 Hz budget for 2,048 particles.

| Path | p50 | p95 | p99 | Mean | Budget |
| --- | ---: | ---: | ---: | ---: | --- |
| Hardware device compute | 2.0 µs | 17.9 µs | 97.4 µs | 5.4 µs | pass |
| Hardware CPU update plus upload | 8.4 µs | 74.2 µs | 105.9 µs | 23.9 µs | pass |
| WARP CPU update plus upload | 3.8 µs | 4.2 µs | 5.0 µs | 4.0 µs | pass |

Device evidence: hardware adapter yes, hardware compute yes, WARP yes. Disabled-GPU startup selected WARP plus CPU upload and never created a second device.

Scalar and compute outputs matched on a test-only staging copy. WARP CPU upload matched the same scalar checksum. The live adapter has no readback method.

An empty `SceneOwner` requests no frames. That is the idle proof. The measure binary's park timeout is wall time and is not process-CPU evidence.

## GPUI patch

Pinned GPUI `797e5dc` creates one hardware `DirectXDevices` in `crates/gpui_windows/src/directx_devices.rs` and already rejects adapters without structured-buffer compute. WARP appears only in atlas tests. The production change is:

1. Pass `SceneDevice` into `DirectXDevices::new`.
2. For `Warp`, call `D3D11CreateDevice(None, D3D_DRIVER_TYPE_WARP, ...)`.
3. Read `HardwarePreference` in `WindowsPlatform::new` before the first create.
4. Remove the 100 ms device-loss sleep. Rebuild once when DXGI reports removal, from the last complete CPU plan.

## rust-gpu

Not adopted. The crate targets SPIR-V, requires nightly, and does not emit SM5 DXBC. Offline FXC remains the asset compiler.

## Limits

- Live TDR and adapter unplug were not forced. Device-loss recovery is proven in the owner types and by `GetDeviceRemovedReason` after each submit.
- Mixed DPI, multi-monitor, and HDR were not part of this run.
- Present time is not in the table. This spike timed update and upload on the admitted device. GPUI already owns present on that device.
- rust-gpu was not compiled. Adding it would pull a second toolchain into a disposable directory without changing the DXBC contract.
