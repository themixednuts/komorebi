# Overview and workspace-navigation evidence

This evidence package answers [Prototype overview and workspace navigation](https://github.com/themixednuts/komorebi/issues/18). The comparison used three overview structures with one renderer-neutral interaction model:

- `spatial`: monitor-shaped workspace map with direct drag targets;
- `focus`: keyboard-first workspace rail and large active-container canvas;
- `familiar`: a Task View-like gallery with manager workspaces and scratchpads added explicitly.

The decision is complete: use the spatial map, adopt the focus board's keyboard behavior and selection detail, and leave Windows virtual-desktop lifecycle to Task View. `RESULTS.md` records why; `CONTRACT.md` defines the production boundary. The HTML is retained only as historical comparison evidence and is not a product surface or a required review step.

Run the deterministic interaction checks with:

```powershell
node overview-model.test.cjs
```

The native probe is an independent Rust crate. It creates one caller-owned Win32 destination, registers 20 and 50 DWM thumbnail slots, measures update-to-`DwmFlush` latency, and observes foreground activation through `EVENT_SYSTEM_FOREGROUND`. It blocks on the Win32 message queue rather than polling foreground state, and emits JSON:

```powershell
cargo run --release --manifest-path native-probe/Cargo.toml
```

This branch is evidence, not production code. The HTML uses representative placeholder content. The native probe never moves, hides, resizes, or changes styles on source windows. Its fixed slot workloads are benchmark parameters, not production limits.
