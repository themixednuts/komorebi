# Measured containment result

## Outcome

The dedicated-process design is practical on the target Windows 11 machine. Keep one Rust + LuaJIT process per extension principal.

The full-hardened run used a unique LPAC SID, `lpacAppExperience`, Win32k disable, child-process restriction, a no-breakaway Job Object, a 256 MiB Job memory ceiling, a 20% CPU hard cap, UI restrictions, a protected host-owned pipe, and a sanitized environment.

## Measurements

| Topology | Cohort | Wall time | Authenticated-ready p50 | Authenticated-ready p99 | Private commit | Echo p99 |
|---|---:|---:|---:|---:|---:|---:|
| Isolated Rust hosts | 1 | 1107.81 ms | 1059.09 ms | 1059.09 ms | 0.85 MiB | 62.80 us |
| Isolated Rust hosts | 4 | 734.14 ms | 633.49 ms | 658.26 ms | 3.36 MiB | 60.00 us |
| Isolated Rust hosts | 16 | 411.58 ms | 216.11 ms | 246.39 ms | 13.46 MiB | 84.60 us |
| Shared LuaJIT control | 16 | 1.18 ms | n/a | n/a | 0.10 MiB incremental | 0.10 us |

The shared control is much cheaper, but one native crash or memory-corruption fault terminates all 16 contexts. The isolated cohort limits that fault to one extension and remains small enough for this personal Windows manager.

One detailed full-hardened run authenticated Rust in 1138.23 ms at 0.84 MiB private commit and LuaJIT in 1294.99 ms at 0.92 MiB. AppContainer profile creation dominates this disposable harness and varies heavily with warm system state; the scale table is the current decision evidence, not a single launch.

## Fault containment

| Fault | IPC observation | Termination | Trigger to observation | Job kill to exit |
|---|---|---|---:|---:|
| LuaJIT-invoked native abort | disconnected, `0xC0000409` | natural crash | 1458.77 ms | n/a |
| CPU loop | deadline | forced Job | 5003.27 ms | 0.93 ms |
| allocation pressure | disconnected, `0xC0000409` | natural abort | 1463.72 ms | n/a |
| deadlock | deadline | forced Job | 5000.39 ms | 2.06 ms |
| indefinite kernel wait | deadline | forced Job | 4995.97 ms | 1.13 ms |
| pipe stall | deadline | forced Job | 5008.49 ms | 1.04 ms |
| clean disconnect | disconnected, `0x00000000` | natural exit | 0.03 ms | n/a |

Every blocking fault stayed inside its one-process Job and the whole Job terminated after the configured native wait deadline. The Lua scenario enters a native callback from LuaJIT and calls the non-unwinding Rust abort primitive; Windows reports the fast-fail process status rather than a Lua error.

## Adversarial observations

Both Rust and LuaJIT children produced the same boundary result:

- The host independently verified the kernel-reported pipe client PID, Job membership, LPAC token property, and exact per-extension AppContainer SID before accepting the child's report.
- Own package data: allowed through an actual NTFS filename containing an unpaired UTF-16 surrogate. The host created it, transferred it through the process environment, and the child opened it without a UTF-8 round trip.
- Host-private file, user profile, SAM, registry, parent-process VM read/write/injection rights, handle duplication, physical drive, other-window messaging, and clipboard: denied.
- A second AppContainer's private file stayed denied both directly and through a native directory-junction reparse point. The protected host file also stayed denied through an extended `\\?\` path.
- A publicly ACL-readable Windows file remained readable through `\\?\`; the prefix does not grant authority or create an isolation boundary.
- Direct IPv4, IPv6, DNS, and UNC network access: denied by the no-network LPAC token.
- Child-process creation: blocked by the process-creation policy.
- Brokered HTTP: status 200 through the host allowlist.
- Brokered private storage: compare-and-swap roundtrip succeeded.
- Forbidden probes unexpectedly allowed: zero at 1, 4, and 16 processes.
- Every tested child exited or was terminated with its Job, and every disposable profile, pipe, junction, and private test file was cleaned up.

Raw evidence is in `results/latest.json`.

The raw report records the exact resolved Cargo dependency tree and represents native paths as optional UTF-8 plus authoritative hexadecimal UTF-16 code units. No tested path is serialized lossily.

## Windows findings

`lpacAppExperience` is necessary runtime compatibility authority, not extension authority. The spike deliberately omitted `registryRead`, `lpacCom`, `lpacClipboard`, and network capabilities and verified those surfaces stayed closed.

The current experimental sandbox-creation API is not suitable as the production base: it remains documented as experimental and is absent from the installed 10.0.28000 SDK. The stable process-attribute path works now.

## Evidence still required before production implementation

The harness now proves CPU, allocation, deadlock, indefinite-wait, pipe-stall, disconnect, and Lua-invoked native-crash containment; stale-generation rejection; bounded malformed/oversized frames; stalled-read cancellation; handle-duplication denial; and direct/cross-extension/reparse-path ACL enforcement.

Production implementation still requires parent-exit and forced-parent-crash kill-on-close evidence, restart/reconnect and restart-budget behavior, IPC backpressure, AF_UNIX comparison, repeated cold/warm distributions, manager responsiveness during faults, durable storage crash recovery/migration/retention/deletion, complete HTTP redirect/DNS-rebinding/private-address/header/MIME/quota/revocation policy, and nested-Job launch contexts. The ticket remains open until those proof obligations are measured or explicitly marked as limitations.
