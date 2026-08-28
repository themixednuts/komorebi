# Measured containment result

## Outcome

The dedicated-process design is practical on the target Windows 11 machine. Keep one Rust + LuaJIT process per extension principal.

The full-hardened run used a unique LPAC SID, `lpacAppExperience`, Win32k disable, child-process restriction, a no-breakaway Job Object, a 256 MiB Job memory ceiling, a 20% CPU hard cap, UI restrictions, a protected host-owned pipe, and a sanitized environment.

## Measurements

| Topology | Cohort | Wall time | Authenticated-ready p50 | Authenticated-ready p99 | Private commit | Echo p99 |
|---|---:|---:|---:|---:|---:|---:|
| Isolated Rust hosts | 1 | 224.89 ms | 172.12 ms | 172.12 ms | 0.85 MiB | 62.80 us |
| Isolated Rust hosts | 4 | 261.68 ms | 180.78 ms | 181.22 ms | 3.38 MiB | 76.70 us |
| Isolated Rust hosts | 16 | 435.23 ms | 223.47 ms | 245.67 ms | 13.51 MiB | 87.30 us |
| Shared LuaJIT control | 16 | 1.07 ms | n/a | n/a | 0.19 MiB incremental | 0.10 us |

The shared control is much cheaper, but one native crash or memory-corruption fault terminates all 16 contexts. The isolated cohort limits that fault to one extension and remains small enough for this personal Windows manager.

One detailed full-hardened run authenticated Rust in 1230.80 ms at 0.84 MiB private commit and LuaJIT in 1178.66 ms at 0.92 MiB. AppContainer profile creation dominates this disposable harness and varies heavily with warm system state; the scale table is the current decision evidence, not a single launch.

## Fault containment

| Fault | IPC observation | Termination | Trigger to observation | Job kill to exit |
|---|---|---|---:|---:|
| LuaJIT-invoked native abort | disconnected, `0xC0000409` | natural crash | 1079.16 ms | n/a |
| CPU loop | deadline | forced Job | 5003.66 ms | 1.38 ms |
| allocation pressure | disconnected, `0xC0000409` | natural abort | 1148.50 ms | n/a |
| deadlock | deadline | forced Job | 4993.46 ms | 1.66 ms |
| indefinite kernel wait | deadline | forced Job | 5003.66 ms | 1.01 ms |
| pipe stall | deadline | forced Job | 4995.25 ms | 1.21 ms |
| clean disconnect | disconnected, `0x00000000` | natural exit | 0.002 ms | n/a |

Every blocking fault stayed inside its one-process Job and the whole Job terminated after the configured native wait deadline. The Lua scenario enters a native callback from LuaJIT and calls the non-unwinding Rust abort primitive; Windows reports the fast-fail process status rather than a Lua error.

## Parent lifetime and restart recovery

An external observer launched a nested containment host and opened the exact LPAC child process handle before allowing the parent to exit. The child had already acknowledged an armed infinite kernel wait, so pipe disconnect could not produce a normal child exit. Both normal parent teardown (`0x00000000`) and an abort without Rust destructors (`0xC0000409`) closed the Job and left the child already signaled when the parent exit became observable; the follow-up waits took 0.0011 ms and 0.0010 ms, respectively. Cleanup removed both AppContainer profiles for each run.

The supervisor then terminated generation 2, consumed its one `RestartPermit`, authenticated generation 3 on a fresh protected pipe, completed the full broker session, rejected a generation 2 frame, and denied a second restart permit. Measured recovery was 581.90 ms.

## IPC backpressure and transport comparison

The protected named-pipe test armed a real LPAC child that intentionally stopped reading. One 49,152-byte payload completed into the 65,536-byte pipe buffer; the next write remained pending and was cancelled at the single 5-second operation deadline (5014.29 ms). The harness settled the exact overlapped operation before releasing its buffer, terminated the child Job, and observed the process tree exit. This is kernel-event-driven overlapped I/O; there is no poll interval or per-channel I/O thread.

A separate full-trust child echoed 32 frames over Windows AF_UNIX with a 32.30 us p99. Both LPAC children were denied while initializing Winsock (`10107`). AF_UNIX also exposes a narrow byte `sun_path`; the probe uses ASCII and rejects arbitrary WTF-16 endpoint names rather than converting them lossily. It provides no public peer PID/token binding equivalent to the protected named-pipe checks. Keep authenticated named pipes for extension IPC.

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

The harness now proves CPU, allocation, deadlock, indefinite-wait, pipe-stall, disconnect, and Lua-invoked native-crash containment; graceful and aborted parent kill-on-close behavior; one-restart recovery; stale-generation rejection; bounded malformed/oversized frames; stalled-read and backpressured-write cancellation; AF_UNIX comparison; handle-duplication denial; and direct/cross-extension/reparse-path ACL enforcement.

Production implementation still requires repeated cold/warm distributions, manager responsiveness during faults, durable storage crash recovery/migration/retention/deletion, complete HTTP redirect/DNS-rebinding/private-address/header/MIME/quota/revocation policy, and explicit nested-Job launch-context coverage. The ticket remains open until those proof obligations are measured or explicitly marked as limitations.
