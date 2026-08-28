# Palette search benchmark

This disposable Rust prototype answers [Benchmark command palette search, indexing, and activation](https://github.com/themixednuts/komorebi/issues/43). It does not implement the palette.

The runner reads the Windows known folders and each explicit `--project-root` without changing them. It creates adversarial files, shortcuts, watcher pressure, and crash fixtures only under a unique temporary directory. The temporary directory is removed when the run settles. The committed JSON contains timings, counts, and sanitized failure classes. It never contains paths, queries, snippets, window titles, application names, AUMIDs, shortcut arguments, or extension payloads.

Run from `harness`:

```powershell
cargo run --release -- run --project-root E:\Projects --output ..\measurements-v3.json
```

The benchmark uses `frizbee = 0.13.0` with `safe_read` and `fff-search = 0.10.5`. Exact versions make the result reproducible. A newer prerelease does not silently change the measured implementation.

The authoritative reference run is `measurements-v3.json`. Exploratory runs were not retained after their fixture and gate-accounting defects were corrected.

Read [DESIGN.md](DESIGN.md) before the code and [RESULTS.md](RESULTS.md) after the run.
