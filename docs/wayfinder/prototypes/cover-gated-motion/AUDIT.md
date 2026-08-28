# Thermo-nuclear code-quality audit

## Verdict

The artifact is suitable as a disposable feasibility probe and evidence generator. It is not production motion code. Its result should be implemented from the typed design in `DESIGN.md`, not copied wholesale into the manager.

## Findings resolved

- Removed an attempted `DwmGetCompositionTimingInfo` field because the returned compositor frame could not be attributed to the cover. Keeping it would have produced authoritative-looking null or false timing evidence.
- Replaced the eight-parameter Win32 creation helper with a `WindowSpec`, concentrating rectangle and style semantics at one boundary.
- Added a nearest-rank p95 regression test. This prevents a 20-sample p95 from being accidentally reported as the maximum.
- Added tile-bound tests for both supported load shapes.
- Kept raw handles and unsafe calls inside `native.rs` and `surface.rs`; serialized domain evidence contains copied values, not live native resources.
- Preserved raw UTF-16 class/title units and made lossy strings diagnostic-only.
- Used RAII for window classes, windows, DWM thumbnails, screen DC release sites, and temporary display-mode restoration.
- Kept the presentation path free of polling, nested runtimes, `block_on`, detached tasks, foreign opacity changes, and per-frame foreign geometry traffic.

## Remaining prototype-only debt

- `surface.rs` combines the disposable renderer, measurement loop, and Win32 resource wrappers. Splitting it would add navigation cost to a throwaway binary; production ownership is already separated in the selected call stack.
- CLI counts are validated at the boundary but stored as `usize` in evidence. Production uses refined `LivePreviewBudget`, `RefreshHz`, generation, deadline, and subject-count types.
- GDI painting and five sentinels provide a conservative mechanism check, not a production GPUI/D3D proof or a whole-frame privacy certificate.
- The probe is synchronous because all measured work is an affinity-bound Win32 loop. Adding Tokio here would manufacture asynchronous structure. Production processes still use one `#[tokio::main]` entrypoint and bounded channels to affinity threads.
- HRESULT translation uses `anyhow` context for a CLI artifact. Production adapters must return closed typed effect and presentation errors.

## Verification

The final artifact passes `cargo fmt`, `cargo check`, two unit tests, and `cargo clippy --all-targets -- -D warnings` with build output redirected off the full E: volume. The four retained v3 evidence files deserialize and contain 120 trials each.
