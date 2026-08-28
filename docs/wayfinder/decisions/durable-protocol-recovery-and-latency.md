# Durable protocol recovery and latency

## Decision

Keep the protocol contract and every latency budget from [Define the versioned local command-catalog protocol](https://github.com/themixednuts/komorebi/issues/23).

Use one file-backed SQLite/WAL/FULL ledger owned by the manager state owner. Reserve invocation identity before admission, commit logical state before dispatch, and record terminal outcome plus committed event atomically. Never replay an ambiguous native effect after a crash.

Publish immutable snapshots after commit. Register a subscription mailbox and capture its snapshot/cursor in one owner operation. Pipe writers consume bounded lanes outside the owner. They never call back into manager state or make publication wait.

Use Drizzle's typed query builders and generated migrations. Store relational recovery facts as columns. Store the versioned action-parameter document as a custom typed BLOB column through `#[column(blob)]` and `DrizzleSQLiteColumn`.

## Alternatives rejected

An in-memory SQLite database with periodic file backup creates two candidate truths and can lose acknowledged state. JSON or JSONL state makes transactions, compaction, conflict checks, and crash recovery application concerns. Both are rejected.

SQLite's internal `jsonb()` representation is also rejected with Drizzle 0.1.16. The crate writes that representation but its generated rusqlite reader tries to decode it as ordinary JSON bytes. A custom BLOB column has one codec and round-trips without SQL strings.

A shared unbounded event channel is rejected. One stalled client would turn memory growth or manager latency into protocol behavior. Separate bounded data and control lanes make overload explicit.

## Typed call stacks

### Invocation admission and effect

```text
named-pipe read completion: &[u8]
  -> pipe::Pipe::receive_frame: FrameHeader + bounded Vec<u8> | PipeError
     [I/O completion event, no polling, 1 MiB allocation ceiling]
    -> frame decoder: Invocation | FrameError
       [strict numeric keys, definite lengths, no trailing bytes]
      -> admit_invocation: Invocation -> Admission
         [state-owner thread, principal authority and capacity check]
        -> DurableStore::recover
           [typed Drizzle select, identity + principal + digest conflict decision]
        -> DurableStore::reserve
           [local-durable SQLite commit, identity becomes retry-visible]
        -> authoritative transition
           [one manager revision and effect plan]
        -> DurableStore::commit_logical
           [local-durable SQLite commit before native dispatch]
        -> effect executor queue
           [asynchronous; manager owner does not wait on the client]
          -> Windows adapter
             [native side effect]
          <- EffectOutcome | adapter error
        -> DurableStore::record_terminal
           [outcome and CommittedEvent in one SQLite transaction]
      <- admitted, retained status, conflict, expired, or capacity-full
    <- numeric-key CBOR response
  <- overlapped named-pipe write completion
```

The durable store owns transactions and idempotency. The Windows adapter owns native error translation. The caller owns neither retries nor reconciliation policy.

### Restart recovery

```text
manager startup
  -> DurableStore::open
     [WAL selected and verified; synchronous FULL; generated migrations]
    -> load nonterminal invocation
      -> Reserved -> RestartedBeforeCommit
      -> LogicalCommitted -> ReconcilingAfterRestart
      -> EffectDispatched + idempotent setter -> observe and converge
      -> EffectDispatched + ambiguous effect -> Indeterminate
      -> Terminal -> retained result
    -> publish Arc<ManagerSnapshot>
```

No recovery branch blindly replays an effect. Observation may prove an idempotent setter converged; a toggle or provider action without proof remains indeterminate.

### Atomic subscription start

```text
subscribe frame + authenticated principal
  -> StateOwner::subscribe
     [single owner operation]
    -> create bounded data lane + reserved control lane
    -> register Subscriber
    -> Arc::clone current ManagerSnapshot
    -> capture EventCursor { epoch, position }
  <- SubscriptionStart { snapshot, cursor, receivers }
```

Publication cannot interleave inside this operation. The next accepted event has global position `S + 1`.

### Publication and slow-reader containment

```text
committed transition
  -> StateOwner::publish
     [hot-local; update immutable snapshot and replay ring]
    -> per-subscriber filter
    -> frame-count and byte-credit check
      -> SyncSender::try_send data
         [never blocks]
      -> full: SyncSender::try_send Lagged on reserved control lane
         -> remove subscriber; require snapshot resynchronization
    -> StateOwner::acknowledge
       [returns frame and byte credit only; rejects duplicate or future sequences]
  <- EventCursor
```

First-party data stops at 1,024 frames or 4 MiB. Extension data stops at 1 MiB. The replay ring stops at 16 MiB or 60 seconds. A new manager epoch invalidates every old cursor.

## Evidence

The release report is [latest.json](../prototypes/durable-protocol-recovery/measurements/latest.json). Every p99 met its budget. Durable admission was 2.961 ms against 16 ms. Publication with 32 stalled readers was 6.3 us against 100 us.

Crash classification, changed digest, changed principal, compaction, atomic start, filtered replay, ring expiry, restart, control-lane lag, parser properties, and principal capacity all passed.

## Production constraints

- Preserve the already-proven explicit current-logon-SID DACL and `PIPE_REJECT_REMOTE_CLIENTS`.
- Keep `InvocationId` globally unambiguous across leased ranges so changing the principal on an existing identity conflicts.
- Keep the manager store on one writer. Publish a new `Arc<ManagerSnapshot>` only after commit.
- Keep action parameter schema/version inside the BLOB document. Do not move identity, revisions, ordering, joins, compaction, or recovery phases into it.
- Add age-based terminal compaction with a minimum 24-hour retention floor.
- Measure an OS-cold handshake after reboot before changing the 30 ms cold budget.
