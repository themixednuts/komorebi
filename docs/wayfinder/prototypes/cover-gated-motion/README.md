# Cover-gated motion feasibility spike

This disposable Windows 11 probe answers [issue #45](https://github.com/themixednuts/komorebi/issues/45). It presents one opaque manager-owned cover, applies two native placement batches behind it, animates DWM thumbnails or privacy-safe geometry placeholders, and measures settlement under 20- and 50-window loads.

The result is conditional, not a blanket approval. The mechanism is viable when a current `MotionCapabilityProfile` admits it and the live cover deadline is met. Otherwise the manager must apply the committed native settlement immediately. The target design does not contain a fixed window-count or refresh-rate allowlist.

Evidence is in the four `measurements-v3-*.json` files. [RESULTS.md](RESULTS.md) summarizes the matrix, [DESIGN.md](DESIGN.md) records the production call stack, and [AUDIT.md](AUDIT.md) records the strict code-quality review.

## Reproduce

Run from `native-probe` on Windows 11:

```powershell
cargo run --release -- inventory
cargo run --release -- matrix --repetitions 20 --refresh 60,120,144,240 --output ../measurements.json --live-limit 20
cargo test
cargo clippy --all-targets -- -D warnings
```

The probe changes the display refresh mode only after `ChangeDisplaySettingsExW(..., CDS_TEST)` succeeds. The synchronous mode change is followed by one `DwmFlush` and one reported-mode verification, not a sleep or settlement loop. Its lease restores the original mode on every normal or error return. It does not poll, install hooks, change foreign opacity, or persist runtime state.

The probe intentionally remains synchronous: it has no asynchronous work to coordinate. Production executables use one `#[tokio::main]` runtime at the binary entrypoint as specified in [DESIGN.md](DESIGN.md).
