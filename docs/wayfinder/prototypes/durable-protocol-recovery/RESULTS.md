# Results

## Decision

Go. Keep the budgets and the durable protocol contract from [Define the versioned local command-catalog protocol](https://github.com/themixednuts/komorebi/issues/23). No budget increase or recovery downgrade is justified.

The reference run on Windows NT 10.0.26200 passed every measured budget and every asserted recovery/flow-control case. The machine had 24 logical processors and the harness working set was 6.81 MiB at observation.

| Operation | p50 | p95 | p99 | max | p99 budget |
| --- | ---: | ---: | ---: | ---: | ---: |
| Warm authenticated handshake | 64.4 us | 138.5 us | 174.3 us | 190.9 us | 10 ms |
| Cold authenticated handshake | 9.867 ms | 11.231 ms | 13.772 ms | 13.772 ms | 30 ms |
| Warm no-op round trip | 18.1 us | 37.1 us | 45.5 us | 178.3 us | 5 ms |
| Project and encode 500 offers | 21.3 us | 27.9 us | 33.5 us | 39.0 us | 8 ms |
| Durable reservation plus logical commit | 1.275 ms | 1.545 ms | 2.961 ms | 5.853 ms | 16 ms |
| Committed event delivery | 0.1 us | 0.2 us | 1.0 us | 11.4 us | 8 ms |
| 1 MiB resnapshot | 721.3 us | 980.5 us | 1.121 ms | 1.236 ms | 50 ms |
| Publication with 32 stalled readers | 1.1 us | 1.6 us | 6.3 us | 161.7 us | 100 us |

The canonical machine-readable report is [measurements/latest.json](measurements/latest.json).

## Recovery

Real child processes exited after each boundary. Reopening the same SQLite database produced:

| Last completed boundary | Recovery result |
| --- | --- |
| None | `NotReserved` |
| Durable reservation | `RestartedBeforeCommit` |
| Logical transition commit | `ReconcilingAfterRestart` |
| Ambiguous effect dispatch | `Indeterminate` |
| Outcome and event commit | `RetainedTerminal` |
| Same identity, changed digest | `IdempotencyConflict` |
| Same identity, changed principal | `IdempotencyConflict` |
| Identity below compacted floor | `InvocationExpired` |

The domain capacity test admits exactly 65,536 live invocations for one principal and rejects the next one.

## Subscription and parser evidence

- Snapshot/cursor capture and mailbox registration happen in one state-owner call. The first subsequent event is cursor position `S + 1`.
- Filtered replay can skip global event positions while per-subscription delivery sequences remain contiguous.
- The replay ring rejects cursors outside either the 16 MiB bound or the 60-second bound.
- A manager restart changes the epoch and always requires a resnapshot.
- Both first-party frame limits and extension byte limits deliver `Lagged` through the reserved control lane, then detach the data subscription.
- Acknowledging delivery 1,024 returned its frame and byte credit; delivery 1,025 then arrived in order. Duplicate and future acknowledgements were rejected.
- Property tests feed arbitrary frame headers and up to 4 KiB of arbitrary CBOR to the parser. Claimed lengths are validated before allocation, indefinite maps and invalid shapes fail, and no input panics.

## Persistence finding

The schema uses Drizzle's custom-column API with `#[column(blob)]`, not SQLite's `jsonb()` encoding. `InvocationDocument` owns the typed serde value and its validated encoded bytes. Drizzle's generated select model invokes the custom BLOB decoder on read.

An earlier attempt used `#[column(JSONB)]`. Drizzle 0.1.16 wrapped insertion in SQLite `jsonb()` but its generated rusqlite row decoder then passed SQLite's internal binary representation to `serde_json`, which failed. The BLOB custom-column API is the coherent round-trip path for this version.

## Quality checks

- `cargo test --all-targets`: 14 passed.
- `cargo clippy --all-targets -- -D warnings`: passed.
- `cargo audit`: no known vulnerabilities; one inherited warning for unmaintained `paste` 1.0.15 (`RUSTSEC-2024-0436`) through Drizzle's dependency graph.
- Release evidence run: go.
- Drizzle generated one baseline migration from the Rust schema.
- No raw SQL, polling loop, retry sleep, lossy native path conversion, `unwrap`, `expect`, or panic is present in the harness.
- The allocation wrapper observed 25,607 allocations, 25,603 deallocations, and four outstanding allocations at report construction.

Stable rustfmt reports that the repository's inherited `imports_granularity` option requires nightly, but formatting completed and `cargo fmt --check` passes.
