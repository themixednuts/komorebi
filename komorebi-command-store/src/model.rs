use std::num::NonZeroU32;
use std::time::Duration;

use komorebi_protocol::InvocationDigest;
use komorebi_protocol::InvocationId;
use komorebi_protocol::InvocationLease;
use komorebi_protocol::InvocationNamespaceId;
use komorebi_protocol::InvocationSequence;
use komorebi_protocol::PrincipalId;
use komorebi_protocol::StateStamp;
use thiserror::Error;

use crate::document::CommittedEventDocument;
use crate::document::InvocationDocument;
use crate::document::OutcomeDocument;

pub const MAX_LIVE_RECORDS_PER_NAMESPACE: i64 = 65_536;
pub const MINIMUM_TERMINAL_RETENTION: Duration = Duration::from_hours(24);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct LedgerTimestamp(i64);

impl LedgerTimestamp {
    /// Creates a nonnegative Unix millisecond timestamp.
    ///
    /// # Errors
    ///
    /// Returns [`TimeError::BeforeUnixEpoch`] for a negative value.
    pub const fn from_unix_millis(value: i64) -> Result<Self, TimeError> {
        if value < 0 {
            Err(TimeError::BeforeUnixEpoch)
        } else {
            Ok(Self(value))
        }
    }

    #[must_use]
    pub const fn as_unix_millis(self) -> i64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum TimeError {
    #[error("ledger timestamps must not precede the Unix epoch")]
    BeforeUnixEpoch,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NamespaceRegistration {
    Registered,
    Existing,
    PrincipalConflict,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LeaseDecision {
    Issued(InvocationLease),
    UnknownNamespace,
    PrincipalConflict,
    CapacityFull,
    SequenceExhausted,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ReservationRequest {
    pub(crate) principal: PrincipalId,
    pub(crate) invocation_id: InvocationId,
    pub(crate) digest: InvocationDigest,
    pub(crate) invocation: InvocationDocument,
    pub(crate) reserved_at: LedgerTimestamp,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Reservation {
    invocation_id: InvocationId,
    digest: InvocationDigest,
}

impl Reservation {
    pub(crate) const fn new(invocation_id: InvocationId, digest: InvocationDigest) -> Self {
        Self {
            invocation_id,
            digest,
        }
    }

    #[must_use]
    pub const fn invocation_id(self) -> InvocationId {
        self.invocation_id
    }

    #[must_use]
    pub const fn digest(self) -> InvocationDigest {
        self.digest
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReservationDecision {
    Reserved(Reservation),
    Retained(InvocationStatus),
    IdempotencyConflict,
    InvocationExpired,
    InvocationNotLeased,
    UnknownNamespace,
    CapacityFull,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecoveryPolicy {
    ObserveAndConverge,
    NeverReplay,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LogicalCommit {
    pub state: StateStamp,
    pub recovery_policy: RecoveryPolicy,
    pub committed_event: CommittedEventDocument,
    pub committed_at: LedgerTimestamp,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DurablePhase {
    Reserved,
    LogicalCommitted,
    EffectDispatched,
    Terminal,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminalKind {
    Succeeded,
    Failed,
    Degraded,
    Indeterminate,
    CancelledBeforeCommit,
    RestartedBeforeCommit,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InvocationStatus {
    pub invocation_id: InvocationId,
    pub digest: InvocationDigest,
    pub phase: DurablePhase,
    pub committed_state: Option<StateStamp>,
    pub terminal_kind: Option<TerminalKind>,
    pub outcome: Option<OutcomeDocument>,
    pub committed_event: Option<CommittedEventDocument>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalRecord {
    pub kind: TerminalKind,
    pub outcome: OutcomeDocument,
    pub recorded_at: LedgerTimestamp,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransitionDecision {
    Applied,
    UnknownInvocation,
    WrongPhase(DurablePhase),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DispatchState {
    NotStarted,
    MayHaveOccurred,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecoveryInvocation {
    pub invocation_id: InvocationId,
    pub state: StateStamp,
    pub policy: RecoveryPolicy,
    pub dispatch: DispatchState,
    pub invocation: InvocationDocument,
    pub committed_event: CommittedEventDocument,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecoveryReport {
    pub reconcile: Vec<RecoveryInvocation>,
    pub restarted_before_commit: Vec<InvocationId>,
    pub indeterminate: Vec<InvocationId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StatusDecision {
    Retained(InvocationStatus),
    InvocationExpired,
    UnknownInvocation,
    UnknownNamespace,
    PrincipalConflict,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompactionBlock {
    NonTerminal(DurablePhase),
    RetentionFloor,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompactionDecision {
    Compacted {
        removed: u32,
        minimum_accepted: InvocationSequence,
    },
    AlreadyCompacted,
    BeyondLeasedRange,
    Blocked {
        invocation_id: InvocationId,
        reason: CompactionBlock,
    },
    UnknownNamespace,
    PrincipalConflict,
    SequenceExhausted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TerminalRetention(Duration);

impl TerminalRetention {
    /// Creates a terminal-retention policy with the protocol's 24-hour floor.
    ///
    /// # Errors
    ///
    /// Returns [`RetentionError::BelowMinimum`] for a shorter duration.
    pub fn new(duration: Duration) -> Result<Self, RetentionError> {
        if duration < MINIMUM_TERMINAL_RETENTION {
            Err(RetentionError::BelowMinimum)
        } else {
            Ok(Self(duration))
        }
    }

    #[must_use]
    pub const fn duration(self) -> Duration {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum RetentionError {
    #[error("terminal invocation retention must be at least 24 hours")]
    BelowMinimum,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LeaseRequest {
    pub namespace: InvocationNamespaceId,
    pub principal: PrincipalId,
    pub count: NonZeroU32,
}
