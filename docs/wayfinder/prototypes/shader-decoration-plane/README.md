# Native decoration-plane feasibility spike

This disposable Windows 11 spike resolves Wayfinder issue #47. It compares a raw D3D11/DXGI/DirectComposition plane with primitives contributed to GPUI's existing Windows composition scene. The production decision is in [DESIGN.md](DESIGN.md); measurements and limitations are in [RESULTS.md](RESULTS.md).

## Decision

Use one shell-role-owned GPUI composition scene. Ship borders, particles, focus transitions, and workspace adornments first as typed GPUI primitives. Add a typed `DecorationPrimitive` to the pinned GPUI renderer when an effect needs a custom pixel shader. Do not ship the standalone D3D11 plane: it would create a second GPU device, swap chain, window, and composition owner for the same decoration.

Lua can instantiate and update bounded effect declarations. It cannot submit code, bytes, handles, buffers, per-frame callbacks, or allocations. A declaration may reference a digest-bound shader asset approved during installation; the runtime consumes only validated DXBC and always has an immediate no-effect fallback.

## Workspace

- `core`: primitive effect, lease, generation, lifetime, and budget types plus pure admission.
- `dcomp-plane`: real D3D11 flip swap chain and DirectComposition visual with HLSL/WGSL asset-route probes.
- `gpui-plane`: pinned GPUI canvas plane in GPUI's existing Windows scene.
- `interaction-target`: controlled cross-process click receiver.
- `probe-runner`: cancel-safe Tokio child supervisor with the workspace's only `#[tokio::main]`.
- `measure-*.ps1`: bounded measurement harnesses. Product code does not poll.

Run from this directory:

```powershell
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace --release
.\measure-interaction.ps1
.\measure-gpu.ps1
```

`measure-explorer-restart.ps1` deliberately restarts Explorer once and should only be run in an interactive test session.
