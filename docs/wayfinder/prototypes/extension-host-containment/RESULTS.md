# Measured containment result

## Outcome

The dedicated-process design is practical on the target Windows 11 machine. Keep one Rust + LuaJIT process per extension principal.

The full-hardened run used a unique LPAC SID, `lpacAppExperience`, Win32k disable, child-process restriction, a standalone inner Job reached through explicit outer-Job breakaway, a 256 MiB Job memory ceiling, a 20% CPU hard cap, UI restrictions, a protected host-owned pipe, and a sanitized environment.

## Measurements

| Topology | Cohort | Wall time | Authenticated-ready p50 | Authenticated-ready p99 | Private commit | Echo p99 |
|---|---:|---:|---:|---:|---:|---:|
| Isolated Rust hosts | 1 | 327.44 ms | 274.72 ms | 274.72 ms | 0.86 MiB | 78.50 us |
| Isolated Rust hosts | 4 | 371.25 ms | 271.52 ms | 272.33 ms | 3.43 MiB | 100.70 us |
| Isolated Rust hosts | 16 | 651.02 ms | 372.78 ms | 411.70 ms | 13.92 MiB | 135.00 us |
| Shared LuaJIT control | 16 | 0.74 ms | n/a | n/a | 0.14 MiB incremental | 0.10 us |

The shared control is much cheaper, but one native crash or memory-corruption fault terminates all 16 contexts. The isolated cohort limits that fault to one extension and remains small enough for this personal Windows manager.

One detailed full-hardened run authenticated Rust in 1206.85 ms at 0.86 MiB private commit and LuaJIT in 1238.38 ms at 0.94 MiB. AppContainer profile creation dominates this disposable harness and varies heavily with system state; the scale table is the latest sample, while the repeated distribution below is the stronger decision evidence.

## Repeated launch distributions

Each process count ran five times with a fresh AppContainer profile and fresh process on every sample. The first observation is retained as descriptive evidence, then four immediate repeats form the resident-cache distribution.

Scale children use the typed `launch_scale` workload: they authenticate, run every containment probe, exercise storage, and measure IPC, but deliberately omit live HTTP so network latency and external availability do not contaminate process-launch evidence. The two detailed runtimes and the restart replacement use `full_broker` and perform fresh, uncached live requests.

| Hosts | Warm samples | Cohort wall p50 | Cohort wall p99 | Ready p99 across samples | Echo p99 across samples | Aggregate commit p99 |
|---:|---:|---:|---:|---:|---:|---:|
| 1 | 4 | 336.77 ms | 346.22 ms | 295.08 ms | 136.60 us | 0.88 MiB |
| 4 | 4 | 365.87 ms | 371.25 ms | 300.18 ms | 165.80 us | 3.44 MiB |
| 16 | 4 | 651.02 ms | 660.22 ms | 435.30 ms | 186.90 us | 13.92 MiB |

All 12 warm cohort samples (84 host processes) exited, allowed zero forbidden probes, and cleaned up their profiles. The raw report retains every sample rather than only the percentiles.

A true OS-cold launch remains an explicit measured-method limitation, not a missing label on the first sample. This process cannot establish a reboot boundary or safely clear Windows' global file/image cache without affecting unrelated applications. A production cold-start claim therefore requires a separate boot-orchestrated measurement; this prototype claims only fresh-profile and resident-cache behavior.

## Fault containment

| Fault | IPC observation | Termination | Trigger to observation | Job kill to exit |
|---|---|---|---:|---:|
| LuaJIT-invoked native abort | disconnected, `0xC0000409` | natural crash | 1317.49 ms | n/a |
| CPU loop | deadline | forced Job | 5012.55 ms | 1.42 ms |
| allocation pressure | disconnected, `0xC0000409` | natural abort | 1311.64 ms | n/a |
| deadlock | deadline | forced Job | 5000.89 ms | 1.37 ms |
| indefinite kernel wait | deadline | forced Job | 5004.20 ms | 1.64 ms |
| pipe stall | deadline | forced Job | 5001.58 ms | 1.13 ms |
| clean disconnect | disconnected, `0x00000000` | natural exit | 0.002 ms | n/a |

Every blocking fault stayed inside its one-process Job and the whole Job terminated after the configured native wait deadline. The Lua scenario enters a native callback from LuaJIT and calls the non-unwinding Rust abort primitive; Windows reports the fast-fail process status rather than a Lua error.

## Host responsiveness during a contained fault

The harness armed a real LPAC CPU-loop child, then put its pipe deadline and Job termination on a dedicated extension-supervision thread while the harness main thread remained the only manager-state owner. An independent requester submitted 64 zero-buffered, revisioned commands. All 64 requests and acknowledgements occurred inside the measured 5003.92 ms armed-fault window; the final manager revision was 65, action round-trip latency was 2.10 us p50 and 14.80 us p99/max, and the fault Job still terminated with the configured `0xDEAD` exit code.

This proves the selected ownership split remains responsive under this CPU-fault workload. It does not claim scheduler behavior for the eventual production manager until that code uses the same separation and is exercised by its vertical tests.

## Parent lifetime and restart recovery

An external observer launched a nested containment host and opened the exact LPAC child process handle before allowing the parent to exit. The child had already acknowledged an armed infinite kernel wait, so pipe disconnect could not produce a normal child exit. Both normal parent teardown (`0x00000000`) and an abort without Rust destructors (`0xC0000409`) closed the Job and left the child already signaled when the parent exit became observable; the follow-up waits took 0.0013 ms and 0.0012 ms, respectively. Cleanup removed both AppContainer profiles for each run.

The supervisor then terminated generation 2, consumed its one `RestartPermit`, authenticated generation 3 on a fresh protected pipe, completed the full broker session, rejected a generation 2 frame, and denied a second restart permit. Measured recovery was 254.70 ms.

## Nested Job contexts

The active Codex launch environment placed the containment host in an outer Job with explicit breakaway permission. The harness classified that context and created each detailed extension with `CREATE_BREAKAWAY_FROM_JOB` before assigning it to the fully restricted inner Job.

A separate outer observer then exercised all four policy combinations with real Job Objects and fresh LPAC sessions:

- No outer UI restriction and no breakaway permission formed a nested inner Job. The extension authenticated, belonged to the inner Job, and exited normally. The inner Job omitted its redundant UI restriction because Windows forbids that setting in a nested chain.
- An outer UI restriction with explicit breakaway permission produced `explicit_breakaway`; the standalone inner Job applied the complete UI policy.
- An outer UI restriction with silent breakaway permission produced `silent_breakaway`; the standalone inner Job applied the complete UI policy.
- An outer UI restriction with neither breakaway mode returned the typed `ui_restrictions_without_breakaway` rejection before extension process creation. This is a Windows Job Object limitation, not a fallback to weaker uncontained execution.

Each helper blocked on a one-byte start gate until the observer completed Job assignment, then completion used `WaitForSingleObject` on the process handle with the configured 30-second deadline. No status polling or settling burst was used.

## IPC backpressure and transport comparison

The protected named-pipe test armed a real LPAC child that intentionally stopped reading. One 49,152-byte payload completed into the 65,536-byte pipe buffer; the next write remained pending and was cancelled at the single 5-second operation deadline (5001.14 ms). The harness settled the exact overlapped operation before releasing its buffer, terminated the child Job, and observed the process tree exit. This is kernel-event-driven overlapped I/O; there is no poll interval or per-channel I/O thread.

A separate full-trust child echoed 32 frames over Windows AF_UNIX with a 20.00 us p99. Both LPAC children were denied while initializing Winsock (`10107`). AF_UNIX also exposes a narrow byte `sun_path`; the probe uses ASCII and rejects arbitrary WTF-16 endpoint names rather than converting them lossily. It provides no public peer PID/token binding equivalent to the protected named-pipe checks. Keep authenticated named pipes for extension IPC.

## Durable broker storage

The host migrated a real schema-1 store to schema 2 through a synced stage and atomic `ReplaceFileW`, rolled it back from the retained backup, and migrated it forward again without losing the legacy value. It then committed a revisioned update, rejected a stale compare-and-swap, enforced a 384 KiB principal quota and 256-entry ceiling with checked arithmetic, rejected a corrupt snapshot one byte beyond its calculated maximum before parsing it, abandoned a fully synced stage to simulate a crash, removed exactly one orphan on reopen, and preserved the last committed value.

Uninstall-with-retention preserved the principal's data. A second principal could not observe the first principal's key. Explicit deletion removed the data, a second deletion converged successfully, and cleanup left no evidence directory. The protocol exposes logical keys and values only; the raw report confirms no backing path was exposed to a child.

## Brokered HTTP

The host fetched `https://example.com/` over a live TLS connection and received status 200, 559 bytes, and `text/html`. The child had no direct Winsock authority; its broker request returned only the status and bounded byte count.

Scripted adversarial transports proved that the broker rejects non-HTTPS URLs, credentials, non-443 ports, hosts outside the exact configured allowlist, non-global DNS answers, a public-to-private DNS change on redirect, excessive redirects, excessive response headers, MIME mismatches, excessive declared or streamed bodies, and aggregate-byte quota exhaustion. A reader that revoked the grant during a body read was rejected before more data could be accepted. The real adapter pins the validated address set into the connection and disables automatic redirects, retries, system proxy discovery, referrers, caller-defined headers, and content decompression; every redirect returns through authorization and DNS resolution.

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

The final run also recovered 43 stale private-probe files left by earlier interrupted prototype runs. The cleanup matched only the complete generated UUID filename grammar inside the manager-owned results directory, did not recurse, and left unrelated artifacts untouched. The final residue count was zero. New private probes now use create-new file creation, so a preexisting path cannot be silently truncated.

Raw evidence is in `results/latest.json`.

The raw report records the exact resolved Cargo dependency tree and represents native paths as optional UTF-8 plus authoritative hexadecimal UTF-16 code units. No tested path is serialized lossily. The code carries native paths as `Path`/`OsStr`; the explicit wide-string boundary round-trips unpaired surrogates and rejects interior NUL. Neither `dunce` nor `normpath` is used to turn a path spelling into authorization evidence.

## Windows findings

`lpacAppExperience` is necessary runtime compatibility authority, not extension authority. The spike deliberately omitted `registryRead`, `lpacCom`, `lpacClipboard`, and network capabilities and verified those surfaces stayed closed.

The current experimental sandbox-creation API is not suitable as the production base: it remains documented as experimental and is absent from the installed 10.0.28000 SDK. The stable process-attribute path works now.

## Evidence still required before production implementation

The harness now proves CPU, allocation, deadlock, indefinite-wait, pipe-stall, disconnect, and Lua-invoked native-crash containment; manager-state responsiveness during a supervised CPU fault; graceful and aborted parent kill-on-close behavior; one-restart recovery; stale-generation rejection; bounded malformed/oversized frames; stalled-read and backpressured-write cancellation; AF_UNIX comparison; handle-duplication denial; direct/cross-extension/reparse-path ACL enforcement; and all supported nested-Job and breakaway launch contexts.

The nested-Job obligation is complete. A true OS-cold launch remains explicitly limited to a future boot-orchestrated measurement; it does not block the containment decision because the warm distribution already establishes the production topology and the cold claim requires a reboot-controlled benchmark outside this process's authority.
