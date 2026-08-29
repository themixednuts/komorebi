# GPU-first decoration compute spike

This disposable Windows 11 spike resolves [Prototype GPU-first decoration compute and WARP fallback](https://github.com/themixednuts/komorebi/issues/49). The call stack is in [DESIGN.md](DESIGN.md). Numbers are in [RESULTS.md](RESULTS.md).

## Decision

One scene device, chosen before the first D3D11 create.

- Hardware plus compute when the adapter reports structured-buffer compute.
- Hardware plus the stable scalar kernel and a bounded upload when compute is off or missing.
- WARP plus that same CPU upload when hardware is disabled or missing.
- `Unavailable` when no device exists or a measured path exceeds the 4.166 ms frame budget.

Warp cannot own device compute. Lua never names a backend. There is no second renderer.

Production patches `gpui_windows::directx_devices::DirectXDevices::new` so it accepts `SceneDevice` instead of always enumerating hardware. This spike measures that same create, dispatch, and upload contract.

## Run

```powershell
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo run --release -p decoration-compute-measure
```
