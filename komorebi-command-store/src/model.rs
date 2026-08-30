use std::num::NonZeroU32;
use std::time::Duration;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use komorebi_protocol::InvocationDigest;
use komorebi_protocol::InvocationId;
use komorebi_protocol::InvocationLease;
use komorebi_protocol::InvocationNamespaceId;
use komorebi_protocol::InvocationSequence;
use komorebi_protocol::InvocationStatus;
use komorebi_protocol::InvocationStatusReply;
use komorebi_protocol::InvocationUnavailable;
use komorebi_protocol::PrincipalId;
use komorebi_protocol::SettledInvocationKind;
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
    /// Reads the current wall-clock time as a ledger timestamp.
    ///
    /// # Errors
    ///
    /// Returns [`TimeError`] when the platform clock is outside the ledger's
    /// nonnegative signed-millisecond range.
    pub fn now() -> Result<Self, TimeError> {
        Self::try_from(SystemTime::now())
    }

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

impl TryFrom<SystemTime> for LedgerTimestamp {
    type Error = TimeError;

    fn try_from(value: SystemTime) -> Result<Self, Self::Error> {
        let duration = value
            .duration_since(UNIX_EPOCH)
            .map_err(|_| TimeError::BeforeUnixEpoch)?;
        let millis = i64::try_from(duration.as_millis()).map_err(|_| TimeError::OutOfRange)?;
        Ok(Self(millis))
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum TimeError {
    #[error("ledger timestamps must not precede the Unix epoch")]
    BeforeUnixEpoch,
    #[error("ledger timestamps must fit a signed 64-bit millisecond clock")]
    OutOfRange,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NewLeaseDecision {
    Issued(InvocationLease),
    NamespaceCollision,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct InvocationCommit {
    pub(crate) principal: PrincipalId,
    pub(crate) invocation_id: InvocationId,
    pub(crate) digest: InvocationDigest,
    pub(crate) invocation: InvocationDocument,
    pub(crate) state: StateStamp,
    pub(crate) recovery_policy: RecoveryPolicy,
    pub(crate) committed_event: CommittedEventDocument,
    pub(crate) committed_at: LedgerTimestamp,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommittedInvocation {
    invocation_id: InvocationId,
    digest: InvocationDigest,
}

impl CommittedInvocation {
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
pub enum InvocationCommitDecision {
    Committed(CommittedInvocation),
    Retained(DurableInvocationRecord),
    IdempotencyConflict,
    InvocationExpired,
    InvocationNotLeased,
    UnknownNamespace,
    CapacityFull,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InvocationInspection {
    Vacant,
    Retained(DurableInvocationRecord),
    IdempotencyConflict,
    InvocationExpired,
    UnknownNamespace,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecoveryPolicy {
    ObserveAndConverge,
    NeverReplay,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DurablePhase {
    Reserved,
    LogicalCommitted,
    EffectDispatched,
    Terminal,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DurableInvocationRecord {
    pub(crate) status: InvocationStatus,
    pub(crate) outcome: Option<OutcomeDocument>,
    pub(crate) committed_event: Option<CommittedEventDocument>,
}

impl DurableInvocationRecord {
    #[must_use]
    pub const fn status(&self) -> InvocationStatus {
        self.status
    }

    #[must_use]
    pub const fn outcome(&self) -> Option<&OutcomeDocument> {
        self.outcome.as_ref()
    }

    #[must_use]
    pub const fn committed_event(&self) -> Option<&CommittedEventDocument> {
        self.committed_event.as_ref()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalRecord {
    pub kind: SettledInvocationKind,
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
    Retained(DurableInvocationRecord),
    InvocationExpired,
    UnknownInvocation,
    UnknownNamespace,
    PrincipalConflict,
}

impl StatusDecision {
    #[must_use]
    pub fn into_reply(self) -> InvocationStatusReply {
        match self {
            Self::Retained(record) => InvocationStatusReply::Retained(record.status),
            Self::InvocationExpired => {
                InvocationStatusReply::Unavailable(InvocationUnavailable::Expired)
            }
            Self::UnknownInvocation => {
                InvocationStatusReply::Unavailable(InvocationUnavailable::UnknownInvocation)
            }
            Self::UnknownNamespace => {
                InvocationStatusReply::Unavailable(InvocationUnavailable::UnknownNamespace)
            }
            Self::PrincipalConflict => {
                InvocationStatusReply::Unavailable(InvocationUnavailable::Forbidden)
            }
        }
    }
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

#[cfg(test)]
mod timestamp_tests {
    use super::*;

    #[test]
    fn system_time_conversion_preserves_epoch_and_rejects_pre_epoch() -> Result<(), TimeError> {
        assert_eq!(
            LedgerTimestamp::try_from(UNIX_EPOCH),
            Ok(LedgerTimestamp::from_unix_millis(0)?)
        );
        let before_epoch = UNIX_EPOCH
            .checked_sub(Duration::from_millis(1))
            .ok_or(TimeError::OutOfRange)?;
        assert_eq!(
            LedgerTimestamp::try_from(before_epoch),
            Err(TimeError::BeforeUnixEpoch)
        );
        Ok(())
    }
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
