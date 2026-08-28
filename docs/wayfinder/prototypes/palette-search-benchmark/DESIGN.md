# Palette benchmark design

## Question and fixed constraints

The benchmark may choose only where the trusted `fff-search` engine runs and the budgets around it. The typed identities, source-specific activation, deterministic palette ranking, local-until-Enter rule, and manager action authority from [Define the first-party command palette and search-source contract](https://github.com/themixednuts/komorebi/issues/37) are fixed.

The process has one Tokio runtime created by `#[tokio::main]`. Pure routing, fencing, bounds, classification, and ranking stay synchronous. Windows COM apartments and directory notification waits keep their required thread affinity. Blocking `fff-search` scans and queries never occupy a Tokio worker. No application code calls `block_on`, polls an atomic, or retries on a timer.

Actual roots are read-only. The probe owns only a unique temporary fixture and the final report. Paths and native strings remain `Path`, `PathBuf`, `OsStr`, `OsString`, or UTF-16 at Windows boundaries. A failure to represent a path in a dependency is evidence, not permission to convert it lossily.

## Candidate process designs

### Candidate A: palette-process actor

One dedicated blocking actor thread owns every `FilePicker`. It builds a complete replacement and swaps it only after success. Content grep receives an abort flag. File-name search has no `fff-search` cancellation input, so a generation fence can reject late output but cannot reclaim a stuck query or an unsafe dependency crash.

This candidate minimizes startup and IPC cost. It loses the entire palette process on a dependency crash, cannot enforce a hard file-name deadline, and shares `fff-search` global pools and allocator behavior with rendering.

### Candidate B: contained long-lived worker

One trusted first-party worker process owns the `FilePicker` snapshots. Private inherited anonymous pipes carry bounded CBOR frames; no globally discoverable pipe name exists. The host verifies the child's PID at the handshake and correlates every reply with a monotonic `RequestId`. A kill-on-close Job Object owns the process lifetime. The dependency's content abort flag handles cancellation admitted before or during a query once the production control lane is present. A missed deadline, stuck file-name search, crash, partial frame, or protocol violation closes the Job and replaces the worker. Engine, worker, root, snapshot, query, and request generations fence every publication.

This candidate pays process startup and framed IPC once. It keeps the palette responsive, contains dependency crashes, reclaims uncancellable work, and leaves command, application, and web routes available while files restart.

The benchmark selects a candidate from measured startup, warm-query latency, memory, disk I/O, cancellation, restart, and cleanup evidence. A contained worker wins only if it stays inside the palette gates and its extra steady cost is reasonable on this machine.

## Typed contracts

```rust
struct EngineEpoch(NonZeroU64);
struct WorkerGeneration(NonZeroU64);
struct RootId(NonZeroU32);
struct SnapshotGeneration(NonZeroU64);
struct QueryGeneration(NonZeroU64);
struct RequestId(NonZeroU64);

struct PublicationFence {
    engine: EngineEpoch,
    worker: WorkerGeneration,
    root: RootId,
    snapshot: SnapshotGeneration,
    query: QueryGeneration,
}

enum FileRequest {
    Build { root: RootSpec, snapshot: SnapshotGeneration },
    SearchName { fence: PublicationFence, query: SearchText, limit: ResultLimit },
    SearchContent { fence: PublicationFence, query: SearchText, limits: ContentLimits },
    Cancel { request: RequestId },
    Shutdown,
}

enum WorkerSettlement {
    Published(BoundedRows),
    Cancelled,
    Stale,
    TimedOut,
    Restarted { next: WorkerGeneration },
    Failed(SanitizedSearchFailure),
}
```

`SearchText` is normalized matching text with a byte limit. Display text remains separate and never crosses the measurement boundary. `BoundedRows`, `ResultLimit`, and `ContentLimits` make row, byte, snippet, file-size, and deadline limits valid before dispatch. `PublicationFence::admits` is the only operation that turns worker output into visible rows.

Shell discovery returns a generation-bound `ShellItemToken`, not a path, PIDL, AUMID string, or callback. A classic activation fixture uses `ShellExecuteExW` once. A packaged token selects the `IApplicationActivationManager` route, but the benchmark does not launch an owner-installed application merely to produce a timing sample.

## Entrypoint-to-effect stacks

### Bounded command and application retrieval

```text
CLI run: RunArgs
  -> benchmark::run: BenchmarkPlan | PlanError
    -> catalog::Catalog::build: bounded normalized Catalog
      -> catalog::search_scores: SearchText -> Vec<ScoredCatalogItem>
        -> frizbee::Matcher::match_list with safe_read [blocking actor]
      -> catalog::highlight_visible: visible handles only
        -> frizbee::Matcher::match_one_indices
    <- redacted latency distribution and invariant outcomes
  <- AtomicReport::publish
```

The catalog owns normalization, bounds, stable item indices, and the score-only then visible-highlight split. Frizbee scores never cross into shared palette ranking semantics.

### File snapshot build and query

```text
BenchmarkPlan: admitted RootSpec
  -> root::inspect: RootAdmission | RootRejection
    -> in-process actor or worker request: Build
      -> fff::Snapshot::build [dedicated thread/process]
        -> FilePicker::new(watch=false, follow_symlinks=false)
        -> FilePicker::collect_files [read-only filesystem walk]
      <- complete SnapshotReady | sanitized BuildFailure
    -> native watcher invalidation [ReadDirectoryChangesW completion]
      -> build a complete replacement snapshot
      -> publish only after success
    -> SearchName | SearchContent
      -> dependency call with bounded options
      -> copy only bounded, redacted evidence out of dependency borrows
    <- PublicationFence::admits | Stale
  <- measurements and cleanup settlement
```

`fff-search` owns its internal in-memory index. The adapter owns root admission, native watcher overflow recovery, immutable replacement, deadlines, cancellation, redaction, and generation fences. The report must not call an in-memory index an on-disk index. If no persistent index exists, corruption and schema mismatch cannot occur; restart rebuild is the recovery contract.

### Contained worker failure

```text
Palette file operation
  -> WorkerClient::request: fenced bounded request
    -> framed inherited anonymous pipes [async, exact spawned child]
      -> worker actor [blocking dependency call]
    <- response | EOF | deadline | protocol failure
  -> deadline owner cancels cooperative content work
  -> missed deadline or process failure closes Job Object
  -> kernel terminates process tree
  -> supervisor awaits exact process exit and starts next generation
  -> stale response fails PublicationFence::admits
  <- Restarted | Failed, never partial rows
```

The request task owns its frame buffer until completion. A timed-out partial read poisons the transport and immediately leads to Job closure; the pipe is never reused. Job closure is the terminal cancellation mechanism for work the dependency cannot cancel. Production adds a concurrent control lane for the content abort flag, while the prototype proves pre-dispatch cooperative abort and hard cancellation for uncancellable or non-acknowledging work.

### Shell identity and activation

```text
Shell STA thread
  -> enumerate FOLDERID_AppsFolder
    -> generation-local PIDL and property reads
    -> ShellItemToken { generation, opaque slot, activation kind }
  <- immutable redacted catalog facts

Enter(ResultHandle)
  -> activation::prepare: current token and one-shot InvocationId
    -> reject stale Shell generation or reused invocation
    -> classic fixture: ShellExecuteExW(open, SW_HIDE)
    -> packaged item: IApplicationActivationManager route
  <- AcceptedByWindows | Stale | OsRejected
```

The COM STA owns PIDLs. The palette sees only tokens. This probe executes 24 hidden classic fixtures with distinct events, preserves duplicate shortcut arguments, and validates the packaged route and live AppsFolder identity without opening a user application.

### Captured-window action

```text
CapturedWindowIdentity { HWND, PID, process birth }
  -> activation::revalidate_once
    -> GetWindowThreadProcessId
    -> open process and GetProcessTimes
    -> compare the complete identity
  -> one-shot activation gate
    -> SetForegroundWindow once when policy permits
  <- Activated | Stale | OsRejected
```

No retry, input synthesis, `AttachThreadInput`, or timer follows a rejected foreground request.

### Web and extension routes

```text
untrusted query text
  -> query::parse: WebDraft | ExplicitExtensionQuery | LocalRoute
    -> local declarative rows only
    -> recording network/browser capabilities remain untouched
  -> Enter with current handle
    -> fixed HTTPS template validation or exact extension grant
    -> one bounded effect call
  <- accepted/rejected settlement
```

The network and browser capabilities are absent from pre-Enter search operations. Tests fail if DNS, HTTP, icon, suggestion, or browser counters change. The existing no-network LPAC extension-host evidence remains the production containment boundary; this benchmark does not duplicate it.

## Failure ownership

- Root admission rejects filesystem roots, implicit full drives, redirected duplicates, disallowed reparse roots, and content roots whose attributes require hydration.
- A dependency path that cannot round-trip through its representation becomes `UnsupportedNativePath`. It is never omitted silently or logged lossily.
- Watcher overflow makes the current snapshot stale and requests one replacement build. Events are invalidation hints, not history.
- Build failure retains the last complete snapshot as stale. There is no partial replacement.
- Query cancellation may stop work cooperatively. Generation fencing remains mandatory because cancellation acknowledgement does not make late output current.
- A worker crash, hang, malformed frame, or missed cancellation deadline closes the Job. The next worker has a new generation.
- Shell enumeration failure retains the last good immutable snapshot as stale. A stale token never activates.
- Report publication uses create-new staging, flush, sync, and same-directory rename. It is evidence output, not application state.

## Proof matrix

The release run records raw distributions and pass/fail outcomes for catalog score-only retrieval, visible highlighting, file scan and query latency, process cost, root policy, ignore rules, reparse and offline handling, native watcher overflow, snapshot replacement, cancellation, stale publication, crash and hang recovery, Job cleanup, AppsFolder refresh, shortcut arguments, classic Shell handoff, captured HWND reuse defense, pre-Enter network silence, extension bounds, and private-data redaction.

The thermo-nuclear audit runs after measurements. It rejects files over 1,000 lines, scattered boolean modes, pass-through layers, unowned tasks, discarded failures, undocumented unsafe blocks, lossy native strings, broad catch-all errors in reusable modules, and any polling loop.
