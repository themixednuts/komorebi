# Measured containment result

## Outcome

The dedicated-process design is practical on the target Windows 11 machine. Keep one Rust + LuaJIT process per extension principal.

The full-hardened run used a unique LPAC SID, `lpacAppExperience`, Win32k disable, child-process restriction, a no-breakaway Job Object, a 256 MiB Job memory ceiling, a 20% CPU hard cap, UI restrictions, a protected host-owned pipe, and a sanitized environment.

## Measurements

| Topology | Cohort | Wall time | Authenticated-ready p50 | Authenticated-ready p99 | Private commit | Echo p99 |
|---|---:|---:|---:|---:|---:|---:|
| Isolated Rust hosts | 1 | 127.72 ms | 83.06 ms | 83.06 ms | 0.83 MiB | 62.40 us |
| Isolated Rust hosts | 4 | 140.30 ms | 74.06 ms | 80.95 ms | 3.32 MiB | 58.00 us |
| Isolated Rust hosts | 16 | 287.41 ms | 123.28 ms | 139.49 ms | 13.30 MiB | 154.40 us |
| Shared LuaJIT control | 16 | 1.04 ms | n/a | n/a | 0.39 MiB incremental | 0.10 us |

The shared control is much cheaper, but one native crash or memory-corruption fault terminates all 16 contexts. The isolated cohort limits that fault to one extension and remains small enough for this personal Windows manager.

One detailed full-hardened run authenticated Rust in 91.67 ms at 0.83 MiB private commit and LuaJIT in 785.70 ms at 0.91 MiB. Startup varied across cold and warm runs; the scale table is the decision evidence, not a single launch.

## Adversarial observations

Both Rust and LuaJIT children produced the same boundary result:

- The host independently verified the kernel-reported pipe client PID, Job membership, LPAC token property, and exact per-extension AppContainer SID before accepting the child's report.
- Own package data: allowed.
- Host-private file, user profile, SAM, registry, parent-process VM read, physical drive, and clipboard: denied.
- Direct IPv4, IPv6, DNS, and UNC network access: denied by the no-network LPAC token.
- Child-process creation: blocked by the process-creation policy.
- Brokered HTTP: status 200 through the host allowlist.
- Brokered private storage: compare-and-swap roundtrip succeeded.
- Forbidden probes unexpectedly allowed: zero at 1, 4, and 16 processes.
- Every tested child exited and every disposable profile, pipe, and private test file was cleaned up.

Raw evidence is in `results/latest.json`.

## Windows findings

`lpacAppExperience` is necessary runtime compatibility authority, not extension authority. The spike deliberately omitted `registryRead`, `lpacCom`, `lpacClipboard`, and network capabilities and verified those surfaces stayed closed.

The current experimental sandbox-creation API is not suitable as the production base: it remains documented as experimental and is absent from the installed 10.0.28000 SDK. The stable process-attribute path works now.

## Evidence still required before production implementation

This spike has not yet proved CPU/allocation/deadlock/native-crash restart behavior, parent-crash kill-on-close, malformed/oversized/stalled-frame cancellation, handle duplication attacks, reparse-point races, durable storage crash recovery/migration/retention, HTTP redirect/DNS-rebinding policy, or nested-Job behavior. Those remain red in the interactive artifact and must be measured before the implementation ticket can rely on this contract.
