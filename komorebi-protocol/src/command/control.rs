use crate::InvocationDigest;
use crate::InvocationId;

use super::StateStamp;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvocationStatusRequest {
    invocation_id: InvocationId,
}

impl InvocationStatusRequest {
    #[must_use]
    pub const fn new(invocation_id: InvocationId) -> Self {
        Self { invocation_id }
    }

    #[must_use]
    pub const fn invocation_id(self) -> InvocationId {
        self.invocation_id
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CancelInvocationRequest {
    invocation_id: InvocationId,
}

impl CancelInvocationRequest {
    #[must_use]
    pub const fn new(invocation_id: InvocationId) -> Self {
        Self { invocation_id }
    }

    #[must_use]
    pub const fn invocation_id(self) -> InvocationId {
        self.invocation_id
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvocationStatus {
    invocation_id: InvocationId,
    digest: InvocationDigest,
    progress: InvocationProgress,
}

impl InvocationStatus {
    #[must_use]
    pub const fn new(
        invocation_id: InvocationId,
        digest: InvocationDigest,
        progress: InvocationProgress,
    ) -> Self {
        Self {
            invocation_id,
            digest,
            progress,
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

    #[must_use]
    pub const fn progress(self) -> InvocationProgress {
        self.progress
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InvocationProgress {
    Reserved,
    LogicalCommitted(StateStamp),
    EffectDispatched(StateStamp),
    Terminal(InvocationTerminal),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InvocationTerminal {
    Settled {
        state: StateStamp,
        kind: SettledInvocationKind,
    },
    CancelledBeforeCommit,
    RestartedBeforeCommit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum SettledInvocationKind {
    Succeeded = 1,
    Failed = 2,
    Degraded = 3,
    Indeterminate = 4,
}

impl SettledInvocationKind {
    pub(super) const fn decode(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::Succeeded),
            2 => Some(Self::Failed),
            3 => Some(Self::Degraded),
            4 => Some(Self::Indeterminate),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InvocationStatusReply {
    Retained(InvocationStatus),
    Unavailable(InvocationUnavailable),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum InvocationUnavailable {
    Expired = 1,
    UnknownInvocation = 2,
    UnknownNamespace = 3,
    Forbidden = 4,
}

impl InvocationUnavailable {
    pub(super) const fn decode(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::Expired),
            2 => Some(Self::UnknownInvocation),
            3 => Some(Self::UnknownNamespace),
            4 => Some(Self::Forbidden),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CancelInvocationReply {
    Cancelled(InvocationStatus),
    TooLate(InvocationStatus),
    AlreadyTerminal(InvocationStatus),
    Unavailable(InvocationUnavailable),
}
