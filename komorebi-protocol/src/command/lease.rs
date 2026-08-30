use std::num::NonZeroU32;

use crate::InvocationLease;
use crate::InvocationNamespaceId;

/// Requests a new namespace or another contiguous range in an existing one.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvocationLeaseRequest {
    namespace: Option<InvocationNamespaceId>,
    count: NonZeroU32,
}

impl InvocationLeaseRequest {
    #[must_use]
    pub const fn new(namespace: Option<InvocationNamespaceId>, count: NonZeroU32) -> Self {
        Self { namespace, count }
    }

    #[must_use]
    pub const fn namespace(self) -> Option<InvocationNamespaceId> {
        self.namespace
    }

    #[must_use]
    pub const fn count(self) -> NonZeroU32 {
        self.count
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InvocationLeaseReply {
    Issued(InvocationLease),
    Rejected(InvocationLeaseRejection),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum InvocationLeaseRejection {
    UnknownNamespace = 1,
    CapacityFull = 2,
    SequenceExhausted = 3,
}

impl InvocationLeaseRejection {
    pub(super) const fn decode(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::UnknownNamespace),
            2 => Some(Self::CapacityFull),
            3 => Some(Self::SequenceExhausted),
            _ => None,
        }
    }
}
