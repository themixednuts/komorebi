# Thermo-nuclear code-quality audit

## Result

The disposable benchmark is acceptable as measured decision evidence. The contained-worker topology is production-worthy; the harness is not a production module to copy wholesale.

Release-blocking findings fixed in the probe:

- `#[tokio::main]` creates the process's only runtime. No code calls `block_on` or creates a nested executor.
- Blocking `fff-search`, COM STA work, directory notification waits, and Shell probes stay off Tokio workers.
- Worker requests and replies are bounded, framed, PID-verified, and correlated by a typed monotonic request ID.
- A cancelled partial frame poisons the transport; Job termination follows and the pipe is never reused.
- Native paths and Shell text remain `Path`, `OsString`, or UTF-16. The dependency's lossy path behavior is reported as a failed invariant rather than hidden.
- Every unsafe Win32/COM operation has a local safety argument. Owned handles and COM apartments settle through RAII.
- Root diagnostics, measurements, and failures contain no raw path, query, snippet, title, AUMID, shortcut argument, or extension payload.
- The watcher uses one blocking kernel notification, not a timer or atomic polling loop. Overflow is an invalidation that requests complete replacement.
- The report is evidence, not state. Same-directory staged publication is write-through and atomic when Windows permits replacement.
- All Rust source files remain below 1,000 lines. Boolean-heavy structures are limited to serialized evidence DTOs and native attribute decomposition, not control flow.

Dependency findings that must stay visible:

- `fff-search` 0.10.5 cannot preserve every legal Windows filename and lacks the native attribute filter required to prevent cloud hydration and hidden/system content access.
- Its filename operation cannot be cooperatively cancelled. The Job boundary is therefore required, not optional defense in depth.
- Its path/content index is memory-resident. Persistent index corruption and schema mismatch are inapplicable; complete rebuild after worker replacement is the recovery operation.
- RustSec finds no known vulnerability. It reports one unmaintained transitive package: `bincode` 1.3.3 through `fff-search -> heed`.

Prototype-only structure:

- `benchmark.rs` and `native.rs` combine the experiment matrix and Windows probes. Production extraction must split root admission, Shell STA discovery, activation, worker supervision, file adapter, and evidence telemetry into separate deep modules.
- The prototype worker processes one dependency request at a time. Production adds a concurrent control lane for cooperative content abort while retaining Job termination for filename work and missed cancellation deadlines.
- The anonymous-pipe transport is correct for one exact spawned child. A future multi-client broker would require a different authenticated transport and is not implied by this decision.

Verification:

```text
cargo test --all-targets                                  10 passed
cargo clippy --all-targets --all-features -- -D warnings PASS
cargo audit                                               0 vulnerabilities; 1 unmaintained transitive warning
cargo tree -e features -i frizbee                         safe_read enabled
release measurement run                                  PASS
continuous polling loops                                 0
orphan worker processes                                  0
```
