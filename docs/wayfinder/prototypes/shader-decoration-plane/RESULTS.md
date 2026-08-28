# Measured results and admission gates

## Result

The native GPU path is feasible, and the single-owner GPUI route is the correct production architecture. Both implementations held approximately 240 Hz on the attached 5120x1440 SDR desktop, passed real cross-process click-through, remained capturable, survived an Explorer restart, and stayed below 1.1% summed per-process GPU-engine utilization during the bounded sample.

The dedicated plane remains measurement scaffolding. It is not a second production renderer.

## Measurements

| Concern | Raw D3D11 + DirectComposition | GPUI existing scene | Decision |
| --- | ---: | ---: | --- |
| Steady frame mean | 4.1708 ms, 1,920 frames/8 s | 4.1697 ms, 1,921 frames/8 s | Equivalent on this 240 Hz display. |
| Steady p99 | 4.2202 ms | 4.2503 ms | Both within one refresh interval. |
| Rapid-motion mean | 4.1803 ms | 4.1676 ms | Both track the moving owned surface. |
| Rapid-motion p99 | 4.2192 ms | 4.4065 ms | GPUI remains within the measured budget. |
| CPU at 4-second sample | 359.375 ms | 593.75 ms | GPUI costs more CPU in the isolated spike; reuse of its existing shell scene avoids adding either full cost as a second renderer. |
| Working set at 4 seconds | 39,698,432 bytes | 50,454,528 bytes | Adopt one scene, not both. |
| Private memory at 4 seconds | 58,499,072 bytes | 74,588,160 bytes | Adopt one scene, not both. |
| Mean summed GPU-engine utilization | 0.9294% | 0.8206% | Four one-second measurement samples. This bounded harness sampling is not a product polling path. |
| Peak summed GPU-engine utilization | 1.0555% | 0.9138% | Same bounded sample. |
| Cross-process click | Target received native click | Target received native click | Both planes were above the target and input-inert. |
| Explorer restart | Alive, normal exit | Alive, normal exit | Both survived one real Explorer restart. |
| Capture | Visible border and particles in screenshot | Visible border in screenshot | Owned surfaces follow normal capture behavior. |

The initial GPUI measurement was approximately 30 Hz because its default no-focus inactive interval was 33.3 ms. Setting `inactive_frame_interval: None` on this dedicated animation surface removed the throttle. Production code requests frames only while an admitted animation is active.

## Shader routes

| Authoring and compilation route | Bytes | D3D11 accepted |
| --- | ---: | --- |
| Hand HLSL -> FXC -> SM5 DXBC | 2,432 | Yes |
| Hand HLSL -> DXC -> SM6 DXIL | 4,440 | No |
| WGSL -> naga HLSL -> DXC -> SM6 DXIL | 3,704 | No |
| WGSL -> naga HLSL -> FXC -> SM5 DXBC | 1,584 | Yes |

The target D3D11 route therefore consumes validated SM5 DXBC. HLSL and WGSL are offline authoring choices, not runtime API formats. Every accepted artifact is digest-bound before activation.

## Display and native evidence

- The attached topology exposed one `DXGI_OUTPUT_DESC1`: raw device name `[92,92,46,92,68,73,83,80,76,65,89,49]` (`\\.\DISPLAY1`), coordinates 0,0-5120,1440, 8 bits per color, DXGI color space 0 (SDR/sRGB).
- Display names were recorded as raw UTF-16/WTF-16 units rather than converted through UTF-8.
- The swap chain used premultiplied B8G8R8A8, flip sequential, and a frame-latency waitable object. The raw loop waited indefinitely for native presentation demand; it did not poll.
- Real `SendInput` testing used Win32 readiness events and a controlled receiver. Control, DComp, and GPUI each resolved `WindowFromPoint` to the target process and delivered the same client click.
- A real Explorer process was stopped and restarted after both planes signaled readiness. Both were alive once the new Explorer reached input-idle and both exited with code 0.
- `DwmGetWindowAttribute(DWMWA_BORDER_COLOR)` returned `E_INVALIDARG` (`-2147024809`). No mutation was attempted. Exact restoration was therefore not claimable, and foreign-border mutation is disabled.

Visual evidence:

- [D3D11/DComp capture](dcomp-decoration-plane-fixed.png)
- [GPUI capture](gpui-decoration-plane-fixed.png)
- [Interaction JSON](interaction-measurements.json)
- [Explorer restart JSON](explorer-restart-measurement.json)
- [GPU sample JSON](gpu-measurement.json)
- [Native capability JSON](native-capabilities.json)

## Explicit admission gates

The attached machine did not expose the following conditions. Production must treat each as unsupported until its capability profile is measured; it must choose an immediate no-effect fallback rather than extrapolate:

- mixed-DPI alignment;
- multiple-monitor synchronization;
- active HDR presentation and SDR/HDR transitions;
- real D3D device removal and recovery under load;
- actual system sleep/resume while effects are active.

The single-monitor SDR run does not prove those rows. Explorer restart, rapid motion, capture, z-order, input transparency, frame pacing, CPU, memory, GPU utilization, and shader-format acceptance were measured.
