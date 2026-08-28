# UI toolkit benchmark prototype

Throwaway comparison for [Benchmark equivalent egui and GPUI Components control surfaces](https://github.com/themixednuts/komorebi/issues/8).

The question is whether the existing egui stack or current GPUI Components stack gives this Windows 11 command palette the better interaction and theming model while meeting measured launch, frame, activation, idle-resource, resize, DPI, keyboard, accessibility, AppBar, and implementation-cost limits.

Both binaries use the same in-memory result identities, filtering, stable selection, and trace format from `palette-prototype-core`. They draw the same 720 x 520 borderless palette with the same data and custom color tokens. They perform no manager action, file launch, or network request.

```powershell
cargo run --release -p palette-egui-prototype
cargo run --release -p palette-gpui-prototype
```

Set `KOMOREBI_UI_PROTOTYPE_TRACE` to a JSONL path to capture process, window, first-frame, query, selection, and activation timestamps.

See [RESULTS.md](RESULTS.md) for the measured Windows 11 comparison and the typed production boundary it supports. Raw samples and pinned revisions are in [measurements.json](measurements.json).

This code is not production code. The branch is the primary-source artifact and will not be merged into the implementation branch.
