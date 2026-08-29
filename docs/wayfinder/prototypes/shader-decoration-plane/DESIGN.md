# Decoration scene and effect API decision

## Chosen ownership

One shell role owns one GPUI window scene, the D3D11 device behind it, all decoration resources, and presentation. The manager owns effect truth. Lua hosts and first-party UI may request effects, but neither renders or keeps an independent scene.

The first implementation slice uses GPUI canvas primitives because the measured border path already reaches 240 Hz in the existing scene. Effects that require a pixel shader extend the pinned GPUI renderer with a closed `DecorationPrimitive` rather than adding another overlay renderer. The custom primitive uses the same GPUI device, command submission, swap chain, DirectComposition tree, device-loss recovery, and transparent surface.

### Alternatives

| Design | Call stack | Result |
| --- | --- | --- |
| GPUI built-ins | typed plan -> GPUI canvas/path/quad -> existing D3D11 scene -> existing swap chain | Adopt first for borders, bounded particles, and adornments. It is the smallest single-owner route. |
| GPUI custom primitive | typed plan -> `DecorationPrimitive` -> GPUI Windows renderer -> existing D3D11 scene -> existing swap chain | Adopt when a measured effect cannot be expressed by built-ins. This is the shader route. |
| Dedicated DirectComposition plane | typed plan -> second D3D11 device -> second swap chain -> second HWND and DComp visual | Reject for production. The spike proves feasibility but duplicates resource, scene, lifecycle, z-order, and input ownership. |

Evidence that could reverse the custom-primitive choice is narrow: a future public GPUI external-render primitive with equivalent ownership, or proof that GPUI cannot meet a specific effect's frame/capture/device-loss contract after the renderer extension is implemented. It would not justify two simultaneous primary renderers.

## Primitive model

The prototype's `decoration-effect-core` crate defines the renderer-neutral vocabulary before any renderer trait:

- `EffectId`: non-zero stable identity.
- `Generation`: non-zero validity epoch with checked advancement.
- `SemanticTarget`: a manager-owned meaning, never a hardcoded HWND or title.
- `EffectParameters`: closed variants for borders, particles, and workspace adornments.
- `EffectLifetime`: target-scoped or bounded fixed duration.
- `EffectBudget`: per-instance particle and texture authority.
- `EffectLease`: the only update/cancel authority returned to a caller.
- `EffectCommand`: spawn, generation-fenced update, or idempotent cancel.
- `SceneUsage` and `SceneBudget`: pure checked admission before renderer work.
- `AssetDigest`: content identity checked against the exact bytes submitted to D3D.

The current personal-profile ceiling is 64 concurrent instances, 2,048 particles, and 32 MiB of effect textures. One instance is limited to 512 particles, 8 MiB of textures, and a 30-second fixed lifetime. These are conservative admission policy, not renderer capacity claims; later measurement may lower or raise them.

## Entrypoint-to-present call stack

### Lua or first-party success path

1. An owner Lua module calls `effects.spawn`, or first-party Rust constructs the same request.
2. The mlua adapter parses Lua tables into the closed `EffectParameters` variants. It resolves the extension principal and digest-bound capability grant before crossing the process boundary. Invalid values remain typed boundary errors.
3. The extension host sends an `EffectCommand` over the authenticated, framed, bounded local protocol. The message contains values only—no callbacks, paths, handles, or renderer objects.
4. The manager checks principal ownership, generation, semantic target, per-instance budget, aggregate `SceneBudget`, and shader-asset grant. It commits the accepted lease before publishing a decoration effect plan.
5. The shell-role adapter receives immutable generation-tagged plans over a bounded channel. Replacing a pending snapshot is safe because the latest complete generation is authoritative; spawn/cancel transitions are not partially applied.
6. On the GPUI thread, the decoration-scene owner atomically replaces the CPU-side plan. Built-ins become canvas primitives. Shader effects become closed `DecorationPrimitive` values containing only approved asset identity and bounded constants.
7. GPUI records the primitives into its existing D3D11 scene and presents through its existing DXGI composition swap chain and DirectComposition visual.
8. The shell reports admitted, presented, degraded-to-no-effect, or device-unavailable against the originating generation. A presented report is not a claim about foreign-window pixels.

### Cancellation and failure path

- A caller cancels an `EffectLease`, a target generation changes, the extension host exits, or its grant is revoked. The manager commits cancellation once. Repeated cancellation converges to the same absent lease.
- Cancellation while a channel send is pending cannot create half a command: the complete value is either accepted or dropped. The next immutable scene plan excludes the lease.
- The GPUI thread never awaits Lua, IPC, disk, shader compilation, or network access. It observes only complete plans at a frame boundary.
- A stale update is rejected by `(EffectId, Generation)` before renderer access.
- Budget, grant, digest, format, or parameter failure produces no effect and leaves the prior valid scene intact.
- Device removal drops GPU resources, presents no effect, and retains only the CPU plan. GPUI's device recovery may rebuild from the latest complete plan; it never replays partial GPU commands.
- Shutdown first closes admission, commits cancellation of all leases, publishes an empty plan, then tears down the owned scene. No foreign appearance state needs restoration.

## Runtime placement

The authoritative manager and each mlua extension host are Tokio processes with one binary-entry `#[tokio::main]`. They never create nested runtimes or call `block_on`. The GPUI shell process owns its GUI thread and GPUI executor and does not create a Tokio runtime; it receives complete typed plans through its existing process adapter. Every spawned async task is owned by a supervisor and cancellation-safe.

Animated effects request renderer frames only while an admitted effect's clock is active. A frame request is presentation demand, not a state-observation poll. With no animated effects, the scene requests no frames. Native readiness and DXGI frame-latency handles drive the spike; there is no timer/equality loop for Windows state.

## GPU compute and software fallback

Effect compute follows the same single-scene ownership. Hardware D3D11 compute updates bounded particle/effect buffers on GPUI's existing device whenever admitted. If GPU compute is disabled, unsupported, or lost, the stable scalar/autovectorized CPU kernel updates a bounded instance buffer and uploads it to the same scene; it does not read GPU state back or create a CPU renderer. If no hardware scene device is usable, the same D3D11 renderer opens as WARP and draws after a CPU update. Failure or an exceeded measured budget yields a typed no-effect degradation while core window management continues.

[Prototype GPU-first decoration compute and WARP fallback](https://github.com/themixednuts/komorebi/issues/49) measured all three live paths under the 240 Hz budget on this machine. Warp cannot own device compute.

Rust-authored shader projects remain behind the closed `DecorationPrimitive` boundary. VectorWare's GPU `core::simd` work is not yet a consumable production toolchain, and `rust-gpu` is early, SPIR-V-oriented, nightly-coupled, and explicitly provides no compatibility guarantee. The first production route therefore remains offline HLSL-to-DXBC inside the pinned GPUI renderer. A future Rust shader compiler may replace asset authorship only after it proves the same accepted bytecode, resource, cancellation, and device-loss contract; it does not change Lua or manager APIs.

## Lua capability contract

The ergonomic surface is declarative:

```lua
local focus_glow = effects.spawn({
  kind = "focus_border",
  target = "focused_window_outline",
  color = { 1.0, 0.16, 0.55, 0.92 },
  width_px = 6.0,
  radius_px = 18.0,
  pulse_hz = 0.8,
})

effects.update(focus_glow, { pulse_hz = 1.2 })
effects.cancel(focus_glow)
```

Strings are accepted only at the Lua adapter and immediately compiled into closed Rust enums. The returned userdata is an opaque `EffectLease` bound to the extension principal and generation. Lua cannot choose identity, generation, native target, frame timing, or resource handles.

Shader-backed declarations may reference a symbolic asset granted in the extension manifest. Installation compiles owner-approved HLSL, or WGSL through naga-generated HLSL, to shader-model-5 DXBC in an isolated build step. It records compiler identity, parameter layout, instruction/resource limits, and SHA-256. Runtime code opens the immutable installed asset once, reads it once, hashes the same bytes it submits, validates the manifest and layout, then calls D3D11. It does not check one path and reopen it later, does not round-trip a Windows path through UTF-8, and does not compile at runtime.

The API never exposes native handles, raw pointers, D3D command buffers, shader bytes, arbitrary constant buffers, per-frame Lua callbacks, unbounded textures, unrestricted particle counts, capture APIs, or foreign-window pixels.

## Native boundary and unsupported effects

DirectComposition effects operate on content in a composition tree the process owns. DWM exposes selected foreign-window attributes, but not an arbitrary foreign-window shader insertion point. Without capture, mirroring, replacing DWM, or application cooperation, arbitrary effects over foreign application pixels are unsupported.

`DWMWA_BORDER_COLOR` is a set operation with no supported readback contract on this Windows 11 target: `DwmGetWindowAttribute` returned `E_INVALIDARG`. Therefore the manager must not modify a foreign border. A future manager-owned lease may mutate an attribute only when its exact baseline is independently known and exact restoration is observed; otherwise the request fails visible and the manager-owned overlay remains the only border route.

Windows paths, environment values, display names, and asset locations remain `OsStr`/`OsString` or native UTF-16/WTF-16 at platform boundaries. Human diagnostics are a separate, explicitly lossy view and never feed identity or I/O.

## Source anchors

- [DCompositionCreateDevice](https://learn.microsoft.com/windows/win32/api/dcomp/nf-dcomp-dcompositioncreatedevice)
- [CreateSwapChainForComposition](https://learn.microsoft.com/windows/win32/api/dxgi1_2/nf-dxgi1_2-idxgifactory2-createswapchainforcomposition)
- [GetFrameLatencyWaitableObject](https://learn.microsoft.com/windows/win32/api/dxgi1_3/nf-dxgi1_3-idxgiswapchain2-getframelatencywaitableobject)
- [DXGI_OUTPUT_DESC1](https://learn.microsoft.com/windows/win32/api/dxgi1_6/ns-dxgi1_6-dxgi_output_desc1)
- [DWM window attributes](https://learn.microsoft.com/windows/win32/api/dwmapi/ne-dwmapi-dwmwindowattribute)
- [Bugs Rust Won't Catch](https://corrode.dev/blog/bugs-rust-wont-catch/)
