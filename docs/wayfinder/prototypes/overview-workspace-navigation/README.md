# Disposable overview and workspace-navigation prototype

This throwaway prototype answers [Prototype overview and workspace navigation](https://github.com/themixednuts/komorebi/issues/18). It compares three overview structures while keeping one renderer-neutral interaction model:

- `spatial`: monitor-shaped workspace map with direct drag targets;
- `focus`: keyboard-first workspace rail and large active-container canvas;
- `familiar`: a Task View-like gallery with manager workspaces and scratchpads added explicitly.

Open `overview-workspace-navigation-prototype.html` directly. Use the fixed bottom switcher, `?variant=spatial|focus|familiar`, or the left and right arrow keys when no control has focus.

Run the deterministic interaction checks with:

```powershell
node overview-model.test.cjs
```

The native probe is an independent Rust crate. It creates one caller-owned Win32 destination, registers 20 and 50 DWM thumbnail slots, measures update-to-`DwmFlush` latency, tests foreground activation from the foreground-owned overview, and emits JSON:

```powershell
cargo run --release --manifest-path native-probe/Cargo.toml
```

This branch is evidence, not production code. The HTML uses representative placeholder content. The native probe never moves, hides, resizes, or changes styles on source windows.
