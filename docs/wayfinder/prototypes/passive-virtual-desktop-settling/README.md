# Passive virtual-desktop settling prototype

This disposable prototype measures a Windows 11 virtual-desktop observer built only from documented APIs. Task View supplies every switch; the probe never enumerates, creates, orders, or switches desktops through private shell COM.

The probe has three commands:

- `target` creates two visible disposable windows and 26 minimized tool windows.
- `observe` samples a quota-balanced 32-HWND cohort through `IVirtualDesktopManager`, DWM cloak state, and ordinary window state. It records HRESULTs, membership, desktop IDs, foreground identity, process CPU, and three-sample settlement.
- `events` captures documented WinEvents for the desktop HWND and tracked windows to identify a zero-idle-query wake source.

The measured design is event-first: `EVENT_OBJECT_NAMECHANGE` on `GetDesktopWindow()` wakes the manager; managed-window cloak/uncloak events corroborate it; a bounded 16 ms public-API burst settles three equal snapshots and then disarms. `EVENT_SYSTEM_DESKTOPSWITCH` is not emitted for Task View virtual desktops.

```powershell
cargo run --release -- target
cargo run --release -- observe --interval-ms 16 --transitions 10 --phase before-explorer-restart --output run-16-before.json
cargo run --release -- events --duration-seconds 30 --output native-events.json
./summarize-results.ps1
```

`CONTRACT.md` contains the primitive-first typed call stack. `RESULTS.md` records the verdict, limitations, and measured evidence. `summarize-results.ps1` deterministically regenerates the statistical tables under `results/`.

The self-contained HTML file is a human-readable logic prototype for the event-first settling machine.

This branch is evidence for [Measure passive Windows virtual-desktop settling](https://github.com/themixednuts/komorebi/issues/38), not production code.
