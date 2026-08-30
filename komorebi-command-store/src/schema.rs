use drizzle::sqlite::prelude::*;

use crate::document::CommittedEventDocument;
use crate::document::InvocationDocument;
use crate::document::OutcomeDocument;
use crate::storage::StoredDigest;
use crate::storage::StoredNamespaceId;
use crate::storage::StoredPrincipalId;
use crate::storage::StoredRevision;
use crate::storage::StoredSequence;

// Integer representations are explicit because these values are durable ABI,
// not declaration-order discriminants.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, SQLiteEnum)]
#[repr(i64)]
pub(crate) enum StoredPhase {
    #[default]
    Reserved = 1,
    LogicalCommitted = 2,
    EffectDispatched = 3,
    Terminal = 4,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, SQLiteEnum)]
#[repr(i64)]
pub(crate) enum StoredRecoveryPolicy {
    #[default]
    ObserveAndConverge = 1,
    NeverReplay = 2,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, SQLiteEnum)]
#[repr(i64)]
pub(crate) enum StoredTerminalKind {
    #[default]
    Succeeded = 1,
    Failed = 2,
    Degraded = 3,
    Indeterminate = 4,
    CancelledBeforeCommit = 5,
    RestartedBeforeCommit = 6,
}

#[SQLiteTable(NAME = "invocation_namespaces", STRICT)]
pub(crate) struct InvocationNamespaces {
    #[column(primary, blob)]
    pub namespace: StoredNamespaceId,
    #[column(blob)]
    pub principal: StoredPrincipalId,
    #[column(blob)]
    pub next_sequence: StoredSequence,
    #[column(blob)]
    pub minimum_accepted: StoredSequence,
    #[column(check = "record_count >= 0 AND record_count <= 65536")]
    pub record_count: i64,
}

#[SQLiteTable(NAME = "invocation_records", STRICT)]
pub(crate) struct InvocationRecords {
    #[column(primary, blob)]
    pub namespace: StoredNamespaceId,
    #[column(primary, blob)]
    pub sequence: StoredSequence,
    #[column(blob)]
    pub principal: StoredPrincipalId,
    #[column(blob)]
    pub digest: StoredDigest,
    #[column(blob)]
    pub invocation: InvocationDocument,
    #[column(integer, enum)]
    pub phase: StoredPhase,
    #[column(integer, enum)]
    pub recovery_policy: Option<StoredRecoveryPolicy>,
    #[column(blob)]
    pub logical_revision: Option<StoredRevision>,
    #[column(integer, enum)]
    pub terminal_kind: Option<StoredTerminalKind>,
    #[column(blob)]
    pub outcome: Option<OutcomeDocument>,
    #[column(blob)]
    pub committed_event: Option<CommittedEventDocument>,
    #[column(check = "reserved_at_ms >= 0")]
    pub reserved_at_ms: i64,
    pub logical_committed_at_ms: Option<i64>,
    pub effect_dispatched_at_ms: Option<i64>,
    pub terminal_at_ms: Option<i64>,
}

#[derive(Debug, SQLiteFromRow)]
#[from(InvocationRecords)]
pub(crate) struct RecoveryCandidate {
    pub namespace: StoredNamespaceId,
    pub sequence: StoredSequence,
    pub invocation: InvocationDocument,
    pub phase: StoredPhase,
    pub recovery_policy: Option<StoredRecoveryPolicy>,
    pub logical_revision: Option<StoredRevision>,
    pub committed_event: Option<CommittedEventDocument>,
}

#[derive(Debug, SQLiteFromRow)]
#[from(InvocationRecords)]
pub(crate) struct InvocationSnapshot {
    pub namespace: StoredNamespaceId,
    pub sequence: StoredSequence,
    pub principal: StoredPrincipalId,
    pub digest: StoredDigest,
    pub phase: StoredPhase,
    pub logical_revision: Option<StoredRevision>,
    pub terminal_kind: Option<StoredTerminalKind>,
    pub outcome: Option<OutcomeDocument>,
    pub committed_event: Option<CommittedEventDocument>,
}

#[derive(Clone, Copy, Debug, SQLiteFromRow)]
#[from(InvocationRecords)]
pub(crate) struct InvocationPhase {
    pub phase: StoredPhase,
}

#[derive(Clone, Copy, Debug, SQLiteFromRow)]
#[from(InvocationRecords)]
pub(crate) struct CompactionCandidate {
    pub namespace: StoredNamespaceId,
    pub sequence: StoredSequence,
    pub phase: StoredPhase,
    pub terminal_at_ms: Option<i64>,
}

#[SQLiteIndex]
pub(crate) struct InvocationRecoveryIdx(InvocationRecords::phase);

#[SQLiteIndex]
pub(crate) struct InvocationCompactionIdx(
    InvocationRecords::namespace,
    InvocationRecords::terminal_at_ms,
);

// Drizzle's derive emits an explicit `Clone` implementation for this `Copy`
// schema handle; the generated implementation is outside our control.
#[allow(clippy::expl_impl_clone_on_copy)]
#[derive(SQLiteSchema)]
pub(crate) struct CommandStoreSchema {
    pub namespaces: InvocationNamespaces,
    pub records: InvocationRecords,
    pub recovery_idx: InvocationRecoveryIdx,
    pub compaction_idx: InvocationCompactionIdx,
}
