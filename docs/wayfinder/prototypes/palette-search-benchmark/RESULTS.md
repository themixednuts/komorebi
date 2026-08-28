# Palette search benchmark results

## Decision

Use one long-lived, first-party contained file-search worker. Private inherited pipes carry bounded CBOR frames with monotonic request correlation. A kill-on-close Job Object owns the worker tree. The shell process retains query routing, merging, selection, Shell discovery, activation, and manager authority.

The process boundary costs less than 0.5 ms p95 over the measured `fff-search` filename call while containing a deliberate abort, a hung query, malformed or partial framing, and the dependency's uncancellable filename path. Every measured worker settled through process completion, with zero orphans.

This placement decision is final for the first production slice. Production file-search budgets are not yet ready to freeze because the broad `E:\Projects` root misses the filename gate and `fff-search` 0.10.5 violates two native Windows admission requirements. Those are adapter/upstream work, not reasons to move unsafe dependency work into the shell process.

## Reference environment and measurements

The authoritative run is [`measurements-v3.json`](measurements-v3.json), produced on Windows x86-64 with 24 logical processors and Rust 1.97.1. All roots are represented only by stable salted tags.

| Measurement | Result |
| --- | ---: |
| `frizbee` score-only, 2,048 items, p95 / p99 | 0.063 / 0.069 ms |
| Visible-row highlight, p95 / p99 | 0.007 / 0.011 ms |
| Worker startup | 28.27 ms |
| `E:\Projects` files / directories | 634,176 / 86,029 |
| Worker snapshot build, warm filesystem cache | 2.216 s |
| Worker filename call, p95 / p99 | 55.68 / 60.23 ms |
| End-to-end worker filename, p95 / p99 | 55.85 / 60.53 ms |
| Measured p95 framing and scheduling increment | 0.16 ms |
| Worker content first batch, end to end | 24.39 ms |
| Loaded worker working set / private bytes | 123.9 / 188.1 MB |
| Loaded worker peak working set during build | 300.2 MB |
| Normal / crash / hang Job cleanup | 7.63 / 2.32 / 2.90 ms |
| Async classic Shell handoff, p95 / p99 | 0.330 / 0.334 ms |
| AppsFolder refresh | 381.79 ms |

`SEE_MASK_ASYNCOK` is required for the Shell adapter. Without it, the same classic shortcut path synchronously waited hundreds of milliseconds for Shell/DDE settlement. With it, all 24 probes signaled and the handoff stayed far below the 50 ms gate.

The broad project root fails the 25 ms filename gate at 52.09 ms p95 in the host and 55.85 ms end to end in the worker. Root partitioning or dependency index/query work must be selected by calibration against the latency gate; a hardcoded path or file-count threshold is not justified by this run. AppsFolder enumeration likewise belongs on its COM STA in the background and publishes a complete immutable generation; it must never run on palette-open. The packaged activation-manager COM route instantiated successfully without launching an installed application.

## Correctness and containment

- A real Git fixture honored `.gitignore` and `.ignore`; ignored content did not enter the snapshot.
- The old immutable snapshot did not observe a newly created file. A complete replacement did.
- Descendant reparse traversal stayed disabled.
- A native `ReadDirectoryChangesW` event invalidated the snapshot with no polling. Injected overflow took the same stale-and-complete-rebuild path.
- Immediate content cancellation was observed. Filename search has no dependency cancellation input, so its missed deadline terminates the worker and invalidates the generation.
- Delayed output, rapid replacement, worker restart, and repeated cancellation produced zero stale publications.
- A deliberate process abort and a non-settling request were contained and reaped; the replacement worker had a new process and worker generation.
- Pre-Enter web and extension search recorded zero DNS, HTTP, remote-icon, suggestion, or browser effects. Browser authority was touched only by explicit Enter.
- Extension row, byte, snippet, cancellation, crash, and generation bounds all rejected the adversarial fixture.
- Shortcut arguments remained distinct for two shortcuts targeting the same executable. Stale Shell tokens were rejected. Captured foreground identity revalidated HWND, PID, and process birth before one foreground attempt.

## Production budgets

The first implementation should enforce these measured ceilings as typed policy:

- warm command/application retrieval: 8 ms p95;
- file worker framing and scheduling increment: 2 ms p95;
- worker ready handshake: 50 ms;
- filename first batch: 25 ms p95, with generation rejection and terminal Job cancellation when the dependency cannot stop;
- content first batch: 50 ms p95, one active content query per session, cooperative abort followed by terminal Job cancellation if it does not settle;
- Shell handoff: 50 ms p95 with `SEE_MASK_ASYNCOK`;
- worker steady working set: 160 MB and private bytes: 256 MB for a calibrated root set near this 634k-file workload;
- worker build peak working set: 384 MB and cached-filesystem process-cold build wall time: 5 s; a clean-reboot filesystem-cold ceiling remains unfrozen because this read-only run did not evict unrelated global file cache;
- persistent path/content-index disk: 0 bytes for this dependency version; no JSON or SQLite index shadow is introduced;
- idle worker CPU and disk activity: zero absent a native invalidation, query, or explicit rebuild.

The 16.7 ms query-replacement visibility target is enforced by immediate generation change and stale-result removal, not by waiting for dependency cancellation.

## Required dependency work before production

1. The invalid-WTF-16 fixture was created on NTFS but did not round-trip through `fff-search`. Its walker stores lossy UTF-8 strings. Fork or upstream a native `OsString`/UTF-16 path representation, or reject the affected root visibly; never silently omit or repair the name.
2. `fff-search` exposes no per-candidate Windows offline, recall-on-open, recall-on-data-access, hidden, or system attribute predicate for content grep. Add a lossless native admission table/filter before any content open so indexing cannot hydrate cloud files or read excluded entries.
3. Calibrate and partition the broad project root until every partitioned query meets 25 ms p95. The partition rule must derive from measurements, not a special case for `E:\Projects`.
4. `fff-search` transitively uses unmaintained `bincode` 1.3.3 through `heed`; RustSec reports no vulnerability, but the contained worker/fork should remove that dependency before the production supply-chain baseline is frozen.
5. Record a clean-reboot build once the adapter fixes above land. Deliberately flushing the owner's global Windows file cache was outside this read-only benchmark's authority.

No durable mutable application state exists in this experiment. SQLite/Drizzle is therefore intentionally absent. JSON is used only for the required immutable evidence report.
