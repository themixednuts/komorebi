use std::num::NonZeroU64;

use thiserror::Error;

use crate::DirectionSequence;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NextSequence {
    Ready(NonZeroU64),
    Exhausted,
}

impl Default for NextSequence {
    fn default() -> Self {
        Self::Ready(NonZeroU64::MIN)
    }
}

impl NextSequence {
    fn take_and_advance(&mut self) -> Result<DirectionSequence, SequenceError> {
        let Self::Ready(next) = self else {
            return Err(SequenceError::Exhausted);
        };
        let issued = DirectionSequence::new(*next);
        *self = NonZeroU64::new(next.get().wrapping_add(1)).map_or(Self::Exhausted, Self::Ready);
        Ok(issued)
    }

    const fn expected(self) -> Option<DirectionSequence> {
        match self {
            Self::Ready(value) => Some(DirectionSequence::new(value)),
            Self::Exhausted => None,
        }
    }
}

/// Owns sequence allocation for one outgoing connection direction.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct OutboundSequence(NextSequence);

impl OutboundSequence {
    /// Issues the next exact sequence number.
    ///
    /// # Errors
    ///
    /// Returns [`SequenceError::Exhausted`] after issuing `u64::MAX`.
    pub fn issue(&mut self) -> Result<DirectionSequence, SequenceError> {
        self.0.take_and_advance()
    }

    #[must_use]
    pub const fn next(&self) -> Option<DirectionSequence> {
        self.0.expected()
    }
}

/// Validates exact contiguity for one incoming connection direction.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct InboundSequence(NextSequence);

impl InboundSequence {
    /// Accepts exactly the next expected direction sequence.
    ///
    /// # Errors
    ///
    /// Returns [`SequenceError::Replay`] for an old sequence,
    /// [`SequenceError::OutOfOrder`] for a gap, or `Exhausted` after the maximum.
    pub fn accept(&mut self, received: DirectionSequence) -> Result<(), SequenceError> {
        let Some(expected) = self.0.expected() else {
            return Err(SequenceError::Exhausted);
        };
        if received.get() < expected.get() {
            return Err(SequenceError::Replay { expected, received });
        }
        if received != expected {
            return Err(SequenceError::OutOfOrder { expected, received });
        }
        let advanced = self.0.take_and_advance()?;
        debug_assert_eq!(advanced, received);
        Ok(())
    }

    #[must_use]
    pub const fn next(&self) -> Option<DirectionSequence> {
        self.0.expected()
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum SequenceError {
    #[error("direction sequence is exhausted")]
    Exhausted,
    #[error("direction sequence replay: expected {expected:?}, received {received:?}")]
    Replay {
        expected: DirectionSequence,
        received: DirectionSequence,
    },
    #[error("direction sequence gap: expected {expected:?}, received {received:?}")]
    OutOfOrder {
        expected: DirectionSequence,
        received: DirectionSequence,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sequence(value: u64) -> Result<DirectionSequence, crate::FrameError> {
        DirectionSequence::try_from(value)
    }

    #[test]
    fn issuer_and_validator_advance_contiguously() -> Result<(), Box<dyn std::error::Error>> {
        let mut outbound = OutboundSequence::default();
        let mut inbound = InboundSequence::default();
        for value in 1..=10_000 {
            let issued = outbound.issue()?;
            assert_eq!(issued, sequence(value)?);
            inbound.accept(issued)?;
        }
        assert_eq!(outbound.next(), Some(sequence(10_001)?));
        assert_eq!(inbound.next(), Some(sequence(10_001)?));
        Ok(())
    }

    #[test]
    fn validator_distinguishes_replay_from_gap_without_advancing()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut inbound = InboundSequence::default();
        inbound.accept(sequence(1)?)?;
        assert!(matches!(
            inbound.accept(sequence(1)?),
            Err(SequenceError::Replay { .. })
        ));
        assert!(matches!(
            inbound.accept(sequence(3)?),
            Err(SequenceError::OutOfOrder { .. })
        ));
        assert_eq!(inbound.next(), Some(sequence(2)?));
        inbound.accept(sequence(2)?)?;
        Ok(())
    }
}
