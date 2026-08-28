# Staged promotion, rollback, and safe stop

## Decision

Treat activation as a durable promotion transaction. Stage and validate the candidate without changing the active installation. During the short cutover, record each completed boundary in the durable manager store. A candidate is authoritative only after `PromotionCommitted` commits. Startup rolls every other promotion state back once to the prior known-good installation. If that rollback cannot become healthy, Safe Stop removes manager-owned effects, stops manager processes, and keeps Explorer available.

Use file-backed SQLite in WAL mode with `synchronous=FULL` as the manager-owned durable store. Use Drizzle's typed query API for application operations and its `build.rs` generator for migrations. Lua remains the owner-facing configuration and extension language; Rust compiles admitted Lua values into normalized internal state in one transaction.

The manager state owner holds the write connection. After commit it publishes an immutable `Arc<ManagerSnapshot>` tagged with that committed revision. Hot reads use the snapshot. The database remains the only durable truth, so there is no periodic backup window, replay-file authority, or startup conflict between memory and disk.

## Primitive contracts

```rust
pub struct PromotionId(NonEmptyAscii);
pub struct StoreRevision(NonZeroU64);
pub struct KnownGoodInstallation(InstallationId);
pub struct StagedInstallation(InstallationId);

pub enum PromotionBoundary {
    Prepared,
    CandidateSealed,
    ConfigurationMigrated,
    WindowSnapshotCaptured,
    InputPaused,
    OwnedShellStopped,
    ActivePointerSwitched,
    CandidateStarted,
    WindowsReconciled,
    InputAndUiStarted,
    HealthAccepted,
    PromotionCommitted,
}

pub enum RecoveryDecision {
    KeepCommitted(StagedInstallation),
    RestoreKnownGood(KnownGoodInstallation),
    SafeStop,
}

pub struct CommittedSnapshot {
    pub revision: StoreRevision,
    pub state: Arc<ManagerSnapshot>,
}
```

The real types should refine identity, revision, and validated installation paths further. These sums matter more than a shared service trait: downstream code cannot confuse “health accepted” with “promotion committed,” and Safe Stop is an explicit state rather than a Boolean combination.

## State ownership

| Owner | Owns | Does not own |
| --- | --- | --- |
| Lua/profile adapter | Owner-authored syntax and plugin declarations | Native handles, durable schema, recovery decisions |
| State owner | One SQLite write connection, transactions, revisions, snapshot publication | Windows effects or renderer state |
| Immutable manager snapshot | Fast read projection of exactly one committed revision | Independent persistence or mutation |
| Promotion operation | Boundary ordering, health deadline, one rollback decision | Database mechanics, process supervision internals |
| Windows adapters | Exact startup pointer, process, AppBar, input, window, and appearance effects | Manager policy or durable truth |
| Recovery composition root | Migration/open, terminal-boundary interpretation, convergence | Normal feature policy or Lua callbacks |

SQLite owns promotion boundaries, compiled internal configuration, candidate seals, recovery placements, native path facts, grants, and health outcomes. The filesystem owns immutable installation payloads and the minimal OS startup reference. Windows owns current foreign-window and shell observations. JSON may be emitted for diagnostics, but it is not a state format.

SQLite JSONB is available through Drizzle's `#[column(JSONB)]` and typed expressions. Use it only for a versioned document that is opaque or normally consumed whole, such as bounded plugin/effect parameters. Keep identities, revisions, foreign keys, ordering, lifecycle state, and recovery predicates in typed columns. Never hide a relational state machine in JSONB.

## Entrypoint-to-effect stacks

### Configuration admission and commit

```text
owner Lua file or authenticated command
  -> Lua/command adapter parses bounded untrusted values
    -> typed compiler produces CandidateConfiguration | AdmissionError
      -> pure transition produces NextManagerState + NativeEffectPlan
        -> state owner begins Drizzle IMMEDIATE transaction
          -> typed inserts/updates + new StoreRevision
          -> SQLite commit under WAL + FULL
        -> atomically publish Arc<ManagerSnapshot> for committed revision
        -> execute generation-fenced native effects
          -> record typed observed outcome in a later transaction
```

Validation and error translation happen at the adapter. The pure transition cannot perform I/O. A failed commit publishes nothing and executes no effect. A failed effect does not roll back committed intent; it produces explicit reconciliation work against the committed revision.

### Promotion

```text
owner invokes promote(StagedInstallation)
  -> validate immutable payload, copied configuration, and candidate seal
    -> persist Prepared through WindowSnapshotCaptured boundaries
      -> pause input and stop owned shell generation
      -> atomically replace exact startup reference
      -> start candidate and reconcile captured windows/appearance
      -> wait on process, IPC, and AppBar readiness events until one deadline
        -> accepted: persist HealthAccepted then PromotionCommitted
        -> rejected/deadline: run one RestoreKnownGood stack
```

The 15-second health budget covers readiness and IPC exchange, not just the final response. Native process handles, overlapped IPC completions, and readiness events wake the operation. There is no timer-driven polling, settlement burst, equality loop, or repeated status query.

### Recovery

```text
recovery process startup
  -> open stable manager-state path
    -> Drizzle-generated migrations in one controlled step
    -> load and validate ordered boundary chain + exact native path facts
      -> PromotionCommitted present: verify candidate convergence
      -> RollbackCompleted present: verify known-good convergence
      -> SafeStopCompleted present: verify safe-stop convergence
      -> otherwise: restore known-good once
        -> rollback health rejected: remove owned effects and safe-stop
```

Recovery operations are idempotent. Each boundary append is conditional on the exact promotion identity and already-completed boundaries. A crash after a native effect but before its boundary causes reconciliation against observed Windows state before the effect is repeated.

### Hot read

```text
manager query / shell snapshot request
  -> load current Arc<ManagerSnapshot>
    -> answer from immutable revision
```

Readers do not acquire the SQLite write connection. Snapshot publication happens only after commit, so no reader can observe state that recovery would discard.

## Native path and trust boundary

Operational Windows paths remain `Path`/`PathBuf`, `OsStr`/`OsString`, or owned UTF-16 at native adapters. The durable store encodes exact little-endian UTF-16 code units as a versioned BLOB because SQLite `TEXT` and Rust `String` cannot carry unpaired surrogates. UI/display strings never flow back into open, compare, launch, authorization, endpoint identity, or persistence.

The SQLite file itself belongs in a stable, valid-Unicode, per-user manager-state directory with a manager-owned ACL. Installation paths—including UNC, verbatim, device-classified, trailing-dot/space, and ill-formed UTF-16 values—are data inside that store. Security-sensitive repeated filesystem work anchors identity on an open handle; path comparison and check-then-open are not authorization.

The prototype uses a synced temporary file and same-volume `MoveFileExW(MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)` for its isolated startup-reference fixture. Production must additionally validate parent ownership/ACLs and use handle-anchored identity where replacement races matter. SQLite's database, WAL, and shared-memory files are one persistence unit and must never be copied independently while live.

## Alternatives

### Resume every unfinished candidate

This preserves forward progress but makes startup reconstruct which irreversible Windows effects happened between the last durable boundary and process death. It also keeps a candidate that never passed the complete health contract. Rejected for the personal continuity contract.

### Roll back every uncommitted promotion

This gives one conservative rule: only durable commit selects the candidate. The known-good payload and copied configuration remain available throughout cutover. Selected. A future resumable promotion would need measured proof that every effect is idempotent or exactly reconcilable before changing this rule.

### In-memory database with periodic file backup

This optimizes the wrong path. It creates acknowledged-but-not-backed-up revisions, two possible startup truths, backup scheduling, and a larger crash protocol. Rejected. An immutable in-memory read snapshot gives equivalent hot-read speed while SQLite owns every acknowledged revision.

### JSON/JSONL state files

Atomic replacement can protect one document, but concurrent features, migrations, indexed recovery queries, relationships, and partial updates would recreate database machinery. JSONL also introduces replay, compaction, and corruption-tail policy. Rejected for manager state. Structured JSON remains acceptable as diagnostic export.

## Measured evidence

The release harness passed all 32 scenarios: six direct health/fault routes, one process-boundary path containing an unpaired UTF-16 surrogate, 12 promotion boundaries, six rollback boundaries, and seven safe-stop boundaries. Failed IPC reached the event-backed 15,000 ms deadline at 15,002.94 ms. Injected-death recovery took 14.14–43.89 ms in the fixture.

The exact results and harness are in `docs/wayfinder/prototypes/staged-promotion-recovery`.

## Verification gates

- Crash after every durable boundary and converge to the selected installation or Safe Stop.
- Kill the candidate before readiness, during IPC, and after health acceptance but before commit.
- Reject invalid copied configuration and changed candidate seals before cutover.
- Detect duplicate AppBar ownership before commit.
- Force rollback rejection and prove manager effects are removed while Explorer remains.
- Corrupt/truncate durable rows, fill the disk, deny directory access, and verify typed failure without a fabricated default.
- Preserve exact UTF-16 code units through persistence and process boundaries; test UNC and verbatim namespaces in production adapters.
- Run concurrent read load against immutable snapshots and write/crash/checkpoint stress against the exact bundled SQLite version.
- Refuse startup if SQLite cannot enter WAL mode. Keep the bundled SQLite at or above a release containing the WAL-reset fix.
- Strict Clippy and thermo-nuclear review reject raw SQL in application code, manual migrations, parallel truth, generic state services, lossy native paths, polling, hidden retry loops, panics, and discarded meaningful errors.

## Supported limits

The harness proves the state machine and fixture effects, not production reparenting, multi-monitor reconciliation, filesystem ACL security, power-loss behavior of every storage device, or recovery from arbitrary database corruption. Windows does not provide a general transaction spanning SQLite, process launch, AppBar registration, window placement, and startup-reference replacement. The design therefore commits intent first, records exact restorable evidence, and reconciles native effects.

## References

- [Drizzle 0.1.16 crate source and typed query API](https://docs.rs/drizzle/0.1.16/drizzle/)
- [Drizzle SQLite JSON expressions](https://docs.rs/drizzle-sqlite/0.1.16/drizzle_sqlite/expr/)
- [SQLite write-ahead logging](https://www.sqlite.org/wal.html)
- [SQLite synchronous pragma](https://www.sqlite.org/pragma.html#pragma_synchronous)
- [Microsoft `MoveFileExW`](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-movefileexw)
- [Corrode: The Bugs Rust Won't Catch](https://corrode.dev/blog/bugs-rust-wont-catch/)
