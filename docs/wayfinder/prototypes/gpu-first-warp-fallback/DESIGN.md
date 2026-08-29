# GPU-first decoration compute design

## Seams under test

These are the public boundaries for this spike. Tests stay on them.

1. `admit` and `enforce_budget` in `core`. Preference, device evidence, and a measured frame cost become one `AdmittedRuntime` or a typed `UnavailableReason`.
2. `AdmittedRuntime::live`. Warp cannot own device compute. That combination does not exist.
3. `SceneOwner`. Latest complete generation replaces a settled plan. Stale generations fail. Cancel of the published generation is idempotent. Device loss keeps the CPU plan and does not invent GPU state.
4. `SceneDevice::open` in `windows-adapter`. One D3D11 device is created as hardware or WARP from a startup choice. The live path has no GPU readback.
5. Particle update equivalence. The GPU compute kernel and the stable scalar kernel produce the same finite results on a test-only staging copy.

Lua, manager state, and first-party UI are not seams here. They send `EffectCommand` values from the existing decoration core and never name a backend.

## Alternatives

| Design | Who owns the device | Why it loses |
| --- | --- | --- |
| Parallel wgpu or raw overlay device | A second GPU owner presents a second tree | Recreates the rejected dedicated plane from the shader decoration spike |
| GPUI hardware device plus a sidecar WARP device | Two devices, live backend races | The disabled-GPU setting must choose a device before creation |
| Always-hardware `DirectXDevices::new` with a later software blit | GPUI stays hardware-only | Cannot express a user disable or a missing adapter without a second renderer |

Selected design: one scene device, chosen once at process start. Compute is a second closed choice on that same device. Hardware compute is allowed only on a hardware device. WARP draws the same instance buffer after a CPU update. Over budget or missing support becomes `Unavailable` and the manager stays live.

Evidence that would reverse this: a documented GPUI public primitive that already owns compute and WARP with the same generation and device-loss contract. That would change the patch site, not the domain types.

## Contracts

```rust
enum HardwarePreference { Enabled, Disabled }

struct DeviceEvidence {
    hardware_adapter: bool,
    hardware_compute: bool,
    warp_device: bool,
}

enum SceneDevice { Hardware, Warp }
enum EffectCompute { DeviceCompute, CpuUpload }

enum AdmittedRuntime {
    Live { device: SceneDevice, compute: EffectCompute },
}

enum UnavailableReason {
    NoSceneDevice,
    OverBudget,
    DeviceRemoved,
}

struct ScenePlan { generation: Generation, particle_count: u16 }
```

`AdmittedRuntime::live(Warp, DeviceCompute)` is a constructor error. Admission never returns that pair.

Lua leak rule: `EffectParameters` and `EffectCommand` stay renderer-neutral. Backend selection is an adapter result, not a request field.

## Success call stack

```text
startup setting or capability probe: HardwarePreference
  -> windows_adapter::probe: DeviceEvidence | ProbeError
    -> core::admit: AdmittedRuntime | UnavailableReason
      -> SceneDevice::open: ID3D11Device + context on the GPUI UI thread
        -> manager EffectCommand: Spawn
          -> decoration-effect-core admission (existing SceneBudget)
            -> SceneOwner::publish(ScenePlan)
              -> GPUI frame demand while the lease is active
                -> DeviceCompute: CS dispatch on the same device
                -> CpuUpload: particle_kernel::step_scalar then UpdateSubresource
                  -> existing GPUI/D3D11 draw + DXGI present + DirectComposition visual
                    -> generation-tagged Presented
```

Production file ownership for the device open is `gpui_windows::directx_devices::DirectXDevices`. Today that function always enumerates a hardware adapter. The patch is to pass `SceneDevice` in and, for Warp, call `D3D11CreateDevice(None, D3D_DRIVER_TYPE_WARP, ...)`. `WindowsPlatform::new` reads the preference before the first device exists. Atlas WARP helpers stay tests. They are not a scene device.

This spike owns the same create/dispatch/upload/present stack in `windows-adapter` so the contract is measured without a Zed clone. It does not present a second tree beside a live GPUI window.

## Failure and change stacks

```text
HardwarePreference::Disabled
  -> admit skips hardware
    -> warp_device: Live { Warp, CpuUpload }
    -> otherwise: Unavailable(NoSceneDevice)

hardware adapter without compute
  -> Live { Hardware, CpuUpload }

measured update or present above FrameBudget
  -> enforce_budget: Unavailable(OverBudget)
    -> SceneOwner stays published as CPU plan
      -> no effect drawn, manager continues

ABN-style device removal or DXGI device-removed
  -> adapter drops GPU resources
    -> SceneOwner::cpu_plan remains
      -> Unavailable(DeviceRemoved) until SceneDevice::open succeeds
        -> rebuild from that plan, never from partial GPU buffers

SceneOwner::publish(stale generation)
  -> PlanError::StaleGeneration
    -> published plan unchanged

SceneOwner::cancel(published generation)
  -> Empty
    -> no frame demand
      -> repeat cancel is Ok

generation replace while a dispatch is in flight
  -> current dispatch finishes or is abandoned with the device context
    -> only the latest complete CPU plan is uploaded or dispatched next
```

No timer, equality loop, or retry sleep belongs in this stack. GPUI's current 100 ms device-loss sleep is a renderer bug to replace with a single native rebuild when DXGI reports removal.

## rust-gpu and VectorWare

`rust-gpu` emits SPIR-V and states it is early, nightly-coupled, and without compatibility guarantees. GPUI and this adapter consume SM5 DXBC. Adding SPIR-V would introduce a compiler and a translation step without removing HLSL. VectorWare's GPU `core::simd` work is not a crate we can depend on.

Shader authorship stays behind `AssetDigest` and offline FXC. A later Rust shader compiler may replace that build step only after it emits the same DXBC contract.

## Tests

- `admit` table for the four live and unavailable outcomes
- constructor rejection of Warp plus DeviceCompute
- generation replace, stale reject, idempotent cancel, loss retains plan
- hardware and WARP device creation on this machine
- scalar vs compute checksum on a test-only staging read
- idle owner requests no frames
