# Passive virtual-desktop settling prototype

This disposable prototype measures whether documented `IVirtualDesktopManager` queries settle predictably after a desktop switch performed through the normal Windows 11 Task View UI. It never enumerates, creates, orders, or switches virtual desktops through private COM.

The probe has two processes:

- `target` creates two visible disposable windows and 26 minimized tool windows so the public API is sampled over a stable set of ordinary and minimized HWNDs.
- `observe` selects a quota-balanced cohort of 32 known top-level windows: 28 probe windows plus packaged, elevated, and ordinary representatives. It polls `GetWindowDesktopId` and `IsWindowOnCurrentVirtualDesktop`, and records per-window HRESULTs, desktop IDs, membership, DWM cloak state, foreground identity, process CPU time, and three-poll settlement.

Task View supplies the switch. The probe uses `GetLastInputInfo` only to timestamp the most recent normal input before public membership changes.

```powershell
cargo run --release -- target
cargo run --release -- observe --interval-ms 16 --transitions 20 --phase before-explorer-restart --output run-16-before.json
```

The HTML file is the human-readable logic prototype. It loads no files and includes the selected state machine plus representative measured runs after the experiment is complete.

`CONTRACT.md` contains the primitive-first Rust call stack and conservative unavailable-observation behavior. `RESULTS.md` is the measurement ledger; raw JSON remains under `results/`.

This branch is evidence for [Measure passive Windows virtual-desktop settling](https://github.com/themixednuts/komorebi/issues/38), not production code.
