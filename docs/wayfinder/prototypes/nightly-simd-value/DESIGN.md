# Toolchain and acceleration decision design

## Primitive model

The spike begins with values, not a compiler service hierarchy:

- `CompilerArm`: stable, pinned nightly default, or pinned nightly with `-Znext-solver`.
- `MeasurementScope`: the source-locked repository or the process-aligned planned-stack fixture.
- `CompilerOperation`: clean check, incremental check, debug build, release build, test, or strict Clippy.
- `ParticleStep`: finite, range-checked timestep, gravity, drag, and bounds.
- `ParticleBatch`: private structure-of-arrays storage whose component lengths cannot diverge through its public API.
- `KernelBackend`: scalar-autovectorized, capability-admitted AVX2, or nightly portable SIMD.
- `ToolchainCapabilityProfile`: evidence for one source revision, target, compiler identity, dependency graph, and machine. It is not a global claim about Rust.

There is no broad `CompilerService`, `SimdProvider`, or renderer-neutral trait added to production. The evidence selected the existing stable scalar primitive, so production needs no new abstraction for this experiment.

## Compiler measurement call stack

1. The runner's single `#[tokio::main]` constructs a fixed Latin-square order of typed compiler arms and scopes.
2. An operation builds a `tokio::process::Command` from the arm and scope. Dependencies and `Cargo.lock` remain identical; only the selected toolchain and explicit solver flag change.
3. `kill_on_drop(true)` binds the child lifetime to the owning future. Ctrl+C cancels the active operation without leaving an unowned compiler process.
4. The completed child produces one `CompilerMeasurement`. Failure is data and does not fabricate success for later operations.
5. The runner checkpoints the whole report to a sibling temporary file, flushes it, and atomically replaces the destination with native `MoveFileExW(MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)`.
6. The just-completed target directory is removed only after its durable result exists. This bounds disk growth without deleting user data or unrelated build output.

Windows paths remain `Path`/`OsStr` and cross the native boundary as UTF-16/WTF-16. Lossy display text is diagnostic-only and never returns to file identity or I/O.

The runner never calls `block_on`, creates a nested runtime, polls a state predicate, or sleeps between observations. Process completion and Ctrl+C are native readiness events. A partial cancelled report remains explicitly `complete: false`.

## Compatibility fixture call stacks

The fixture follows the intended process boundary instead of forcing incompatible native libraries into one executable:

- Manager/state process: typed Drizzle query -> generated migration -> `SQLiteFromRow` -> SQLite Blob column -> durable row.
- Extension-host process: bounded UTF-8 Lua module text -> mlua/LuaJIT -> closed Rust value -> process message. It never carries native UI or database ownership.
- Shell process: GPUI Components value -> GPUI event loop -> Windows scene. It does not own a Tokio runtime.

This split exposed a real build policy: vendored LuaJIT selects the static MSVC runtime. The extension host must be built consistently with `-Ctarget-feature=+crt-static` unless `luajit-src` gains an explicit compatible CRT selection. Combining the shell and LuaJIT in one fixture produced an artificial CRT collision and was rejected because it violated the actual process design.

## CPU kernel call stack

1. The manager admits a bounded effect and constructs a checked `ParticleStep`.
2. The effect backend owns a `ParticleBatch`; callers cannot create mismatched component lengths.
3. The stable production path calls the ordinary scalar loop. LLVM autovectorizes it on the measured target.
4. The experimental AVX2 adapter admits the CPU feature once, encloses the only unsafe target-specific call, and returns to the scalar loop for the tail.
5. Property tests compare scalar and experimental outputs over varied lengths and finite inputs. The same checksum and zero timed allocations are required before timing is considered.

No CPU backend spins while idle. Animated work exists only while an admitted effect requests a frame; inactive benchmark processes use one one-shot park and record zero process CPU.

## GPU-primary effect call stack

The acceleration boundary is an effect-compute choice inside the one decoration scene, not a second graphics architecture:

1. Manager effect intent -> checked `EffectPlan` -> immutable scene generation.
2. GPUI scene owner admits the effect against its resource budget.
3. With hardware acceleration enabled and supported, `HardwareEffectKernel` updates particles on the same D3D11 device used by GPUI, then the closed decoration primitive draws those instances through the same command stream, swap chain, and DirectComposition visual.
4. If compute is disabled, unsupported, or loses its device, `CpuEffectKernel` runs the stable scalar update and uploads a bounded instance buffer to that same scene. It performs no GPU readback.
5. If no hardware scene device is available, the future Windows adapter may create a D3D11 WARP device and keep the same GPUI renderer contract. If WARP creation or its measured budget fails, effects degrade to absent while core window management continues.

The backend state is closed and observable: `Hardware`, `CpuUpdate`, `WarpScene`, `Disabled`, or `Unavailable(reason)`. A generation change or cancellation drops pending work before publication. Device loss invalidates GPU resources, retains only the latest complete CPU plan, and rebuilds from that plan after device recovery.

WARP and the GPUI renderer extension are follow-up measurements, not claims established by this compiler spike. Microsoft documents WARP as the conformant Direct3D software rasterizer and D3D11's `D3D_DRIVER_TYPE_WARP` as the native creation route.

## Rust-authored GPU code decision

Three ideas were separated:

- Rust portable SIMD on CPUs is an unstable library feature and did not win this workload.
- VectorWare's mapping of `core::simd` to GPU warp lanes is compiler research. Its authors state that soundness coverage is incomplete and more exploration is necessary; it is not a production crate or upstream Rust capability.
- `rust-gpu` compiles Rust to SPIR-V, but its project describes itself as early, not production-ready, and without compatibility guarantees. The pinned GPUI Windows renderer is D3D11/HLSL and does not expose a public custom-shader insertion API, so adding SPIR-V would also require translation and a second unstable compiler stack.

Production therefore uses a closed Rust `EffectKernel`/`DecorationPrimitive` contract with offline-compiled HLSL/DXBC assets inside the pinned GPUI renderer. `rust-gpu` may later compile an isolated experimental implementation against the same contract. It cannot leak SPIR-V types, nightly features, raw GPU handles, or shader compiler ownership into Lua, manager state, or the public effect API.

## Sources

- [Nightly portable SIMD](https://doc.rust-lang.org/nightly/std/simd/index.html)
- [Next trait solver rustc interface](https://doc.rust-lang.org/nightly/nightly-rustc/rustc_next_trait_solver/solve/index.html)
- [rust-gpu status and scope](https://github.com/Rust-GPU/rust-gpu)
- [VectorWare: Rust SIMD on the GPU](https://www.vectorware.com/blog/simd-on-gpu/)
- [wgpu architecture](https://github.com/gfx-rs/wgpu)
- [Microsoft WARP guide](https://learn.microsoft.com/windows/win32/direct3darticles/directx-warp)
- [D3D11CreateDevice](https://learn.microsoft.com/windows/win32/api/d3d11/nf-d3d11-d3d11createdevice)
- [Bugs Rust Won't Catch](https://corrode.dev/blog/bugs-rust-wont-catch/)

