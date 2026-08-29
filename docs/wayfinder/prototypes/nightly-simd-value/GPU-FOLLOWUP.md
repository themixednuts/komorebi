# GPU-first effects and fallback follow-up

Tracking ticket: [#49 Prototype GPU-first decoration compute and WARP fallback](https://github.com/themixednuts/komorebi/issues/49).

## Recommendation

Use GPU compute and drawing for particles, shader borders, and animated decoration whenever the owned GPUI D3D11 scene supports it. Keep one renderer and one composition tree.

The fallback ladder is:

1. Hardware D3D11 compute/update plus hardware draw in the GPUI scene.
2. Stable CPU scalar/autovectorized update plus a bounded instance-buffer upload and hardware draw in the same scene.
3. Stable CPU update plus the same D3D11 renderer on WARP when hardware acceleration is disabled or no usable adapter exists.
4. A typed `Unavailable` outcome and no effect if the requested effect exceeds the measured CPU/WARP budget. Core window management remains active.

This separates compute fallback from scene-device fallback. It does not maintain mirrored GPU and CPU renderers, perform readback, run both backends simultaneously, or add a second wgpu/DirectComposition overlay.

## Rust GPU projects

VectorWare's August 2026 work maps `core::simd` lanes to NVIDIA warp lanes and is promising compiler research. Their own report says portable SIMD is still unstable, soundness coverage is incomplete, and the correct shared CPU/GPU representation needs more exploration. There is no consumable upstream Rust or production crate to adopt today.

`rust-gpu` is the relevant Rust shader compiler project, but it currently targets SPIR-V and explicitly describes itself as early, not production-ready, and without compatibility guarantees. Our pinned GPUI Windows renderer consumes D3D11 HLSL/DXBC through private renderer internals. Making `rust-gpu` primary now would add a pinned nightly compiler backend and SPIR-V translation without removing the required GPUI renderer extension.

Keep shader authorship replaceable behind the closed typed effect primitive. Use offline HLSL-to-DXBC for the first production backend. Later, an isolated `rust-gpu` or other Rust shader compiler can prove it emits acceptable assets without changing Lua or manager APIs.

## Required spike

The next implementation ticket must measure, on Windows 11:

- extending the pinned GPUI D3D11 renderer with a closed particle/effect primitive on its existing device and command stream;
- hardware compute versus stable CPU update and bounded upload at the target refresh rates;
- explicit D3D11 WARP device creation, GPUI initialization, feature support, and practical frame/power budgets;
- adapter changes, device removal, cancellation, scene generation replacement, and recreation from the last complete CPU plan;
- a user setting that disables hardware acceleration before device creation, without live backend races;
- identical visual/correctness fixtures across hardware, CPU-update, and WARP paths;
- graceful effect removal when a backend is unavailable or over budget.

No Lua extension receives shader bytes, raw handles, per-frame callbacks, or backend selection authority. Lua declares a bounded effect; Rust admission selects the backend.

## Sources

- [VectorWare: Rust SIMD on the GPU](https://www.vectorware.com/blog/simd-on-gpu/)
- [rust-gpu](https://github.com/Rust-GPU/rust-gpu)
- [wgpu](https://github.com/gfx-rs/wgpu)
- [Microsoft WARP guide](https://learn.microsoft.com/windows/win32/direct3darticles/directx-warp)
- [D3D11CreateDevice](https://learn.microsoft.com/windows/win32/api/d3d11/nf-d3d11-d3d11createdevice)
