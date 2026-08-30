use std::num::NonZeroU32;
use std::num::NonZeroU64;

use thiserror::Error;

macro_rules! opaque_nonzero_bytes {
    ($name:ident, $size:literal, $description:literal) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
        pub struct $name([u8; $size]);

        impl $name {
            /// Creates a nonzero opaque identifier.
            ///
            /// # Errors
            ///
            /// Returns [`InvocationIdentityError::ZeroBytes`] for an all-zero value.
            pub fn new(bytes: [u8; $size]) -> Result<Self, InvocationIdentityError> {
                if bytes == [0; $size] {
                    Err(InvocationIdentityError::ZeroBytes($description))
                } else {
                    Ok(Self(bytes))
                }
            }

            #[must_use]
            pub const fn into_bytes(self) -> [u8; $size] {
                self.0
            }
        }
    };
}

opaque_nonzero_bytes!(PrincipalId, 32, "principal ID");
opaque_nonzero_bytes!(InvocationNamespaceId, 16, "invocation namespace ID");
opaque_nonzero_bytes!(InvocationDigest, 32, "invocation digest");

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct InvocationSequence(NonZeroU64);

impl InvocationSequence {
    #[must_use]
    pub const fn new(value: NonZeroU64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0.get()
    }

    /// Advances by an issued lease count.
    ///
    /// # Errors
    ///
    /// Returns [`InvocationIdentityError::SequenceExhausted`] when the range
    /// has no representable exclusive end.
    pub fn advance(self, count: NonZeroU32) -> Result<Self, InvocationIdentityError> {
        self.get()
            .checked_add(u64::from(count.get()))
            .and_then(NonZeroU64::new)
            .map(Self)
            .ok_or(InvocationIdentityError::SequenceExhausted)
    }

    /// Returns the following sequence.
    ///
    /// # Errors
    ///
    /// Returns [`InvocationIdentityError::SequenceExhausted`] at `u64::MAX`.
    pub fn next(self) -> Result<Self, InvocationIdentityError> {
        self.advance(NonZeroU32::MIN)
    }
}

impl TryFrom<u64> for InvocationSequence {
    type Error = InvocationIdentityError;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        NonZeroU64::new(value)
            .map(Self)
            .ok_or(InvocationIdentityError::ZeroSequence)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct InvocationId {
    namespace: InvocationNamespaceId,
    sequence: InvocationSequence,
}

impl InvocationId {
    #[must_use]
    pub const fn new(namespace: InvocationNamespaceId, sequence: InvocationSequence) -> Self {
        Self {
            namespace,
            sequence,
        }
    }

    #[must_use]
    pub const fn namespace(self) -> InvocationNamespaceId {
        self.namespace
    }

    #[must_use]
    pub const fn sequence(self) -> InvocationSequence {
        self.sequence
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvocationLease {
    namespace: InvocationNamespaceId,
    first: InvocationSequence,
    count: NonZeroU32,
    minimum_accepted: InvocationSequence,
}

impl InvocationLease {
    #[must_use]
    pub const fn new(
        namespace: InvocationNamespaceId,
        first: InvocationSequence,
        count: NonZeroU32,
        minimum_accepted: InvocationSequence,
    ) -> Self {
        Self {
            namespace,
            first,
            count,
            minimum_accepted,
        }
    }

    #[must_use]
    pub const fn namespace(self) -> InvocationNamespaceId {
        self.namespace
    }

    #[must_use]
    pub const fn first(self) -> InvocationSequence {
        self.first
    }

    #[must_use]
    pub const fn count(self) -> NonZeroU32 {
        self.count
    }

    #[must_use]
    pub const fn minimum_accepted(self) -> InvocationSequence {
        self.minimum_accepted
    }

    #[must_use]
    pub fn contains(self, id: InvocationId) -> bool {
        if id.namespace != self.namespace {
            return false;
        }

        let offset = id.sequence.get().checked_sub(self.first.get());
        offset.is_some_and(|offset| offset < u64::from(self.count.get()))
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum InvocationIdentityError {
    #[error("{0} must not be all zeroes")]
    ZeroBytes(&'static str),
    #[error("invocation sequences begin at one")]
    ZeroSequence,
    #[error("the invocation sequence space is exhausted")]
    SequenceExhausted,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn namespace() -> Result<InvocationNamespaceId, InvocationIdentityError> {
        InvocationNamespaceId::new([1; 16])
    }

    #[test]
    fn leases_are_half_open_and_namespace_scoped() -> Result<(), InvocationIdentityError> {
        let lease = InvocationLease::new(
            namespace()?,
            InvocationSequence::try_from(7)?,
            NonZeroU32::new(3).ok_or(InvocationIdentityError::SequenceExhausted)?,
            InvocationSequence::try_from(4)?,
        );

        assert!(lease.contains(InvocationId::new(
            namespace()?,
            InvocationSequence::try_from(7)?,
        )));
        assert!(lease.contains(InvocationId::new(
            namespace()?,
            InvocationSequence::try_from(9)?,
        )));
        assert!(!lease.contains(InvocationId::new(
            namespace()?,
            InvocationSequence::try_from(10)?,
        )));
        assert!(!lease.contains(InvocationId::new(
            InvocationNamespaceId::new([2; 16])?,
            InvocationSequence::try_from(7)?,
        )));
        Ok(())
    }

    #[test]
    fn identifiers_reject_zero_and_sequence_overflow() {
        assert_eq!(
            PrincipalId::new([0; 32]),
            Err(InvocationIdentityError::ZeroBytes("principal ID"))
        );
        assert_eq!(
            InvocationSequence::try_from(0),
            Err(InvocationIdentityError::ZeroSequence)
        );
        assert_eq!(
            InvocationSequence::try_from(u64::MAX).and_then(InvocationSequence::next),
            Err(InvocationIdentityError::SequenceExhausted)
        );
    }
}
