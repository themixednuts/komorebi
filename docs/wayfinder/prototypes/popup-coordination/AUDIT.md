# Thermo-nuclear code-quality audit

## Result

The disposable probe is acceptable as measured evidence. It is not acceptable as a production module copied wholesale into the manager.

Release-blocking findings fixed in the probe:

- Boolean bags were replaced by typed visibility, enabled, provenance, graph, foreground, style-flag, proof, and placement-invariant values.
- Every unsafe Win32/COM call now has a local precondition argument.
- Clippy `all` and `pedantic` pass with warnings denied; unwrap, expect, panic, TODO, undocumented unsafe, and debug macros are denied.
- Native path/argument/class boundaries no longer make lossy conversions or unchecked integer casts.
- Child and hook ownership converge on cancellation; UIA generations reject late results.
- The eager `bool::then_some` fixture bug found by native execution was replaced with lazy control flow.
- Errors that affect experiment meaning are propagated. The remaining ignored cleanup errors occur only in `Drop`, where no error channel exists and kill-on-drop remains armed.

Prototype-only debt that must not cross into production:

- `domain.rs` and `report.rs` are large because they combine an exhaustive experiment matrix, evidence schema, and property tests. Production extraction must split observation, classification, modal guard, placement planning, UIA supervision, and evidence telemetry into separate deep modules.
- The WinEvent callback uses a process-global single-observer cell. That matches one disposable run and one manager hook, but production ownership should be an explicit manager singleton with startup failure surfaced through the owner loop.
- The harness uses `anyhow` at executable and adapter boundaries. Production ports should expose closed error enums and convert Win32/COM errors at the boundary.
- Controlled role fixtures are declarative strings in evidence code. Production policy must use typed configured roles and must never identify third-party behavior from a process name or title.

Verification:

```text
cargo clippy --all-targets --all-features -- -D warnings  PASS
cargo test --all-targets                                  5 passed
cargo audit                                               0 known vulnerabilities
release native evidence run                               PASS
```
