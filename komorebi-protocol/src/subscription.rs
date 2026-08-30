use std::collections::BTreeSet;
use std::num::NonZeroU16;
use std::num::NonZeroU64;

use thiserror::Error;

use crate::ManagerEpoch;

const MAX_FILTER_TOPICS: usize = 256;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct EventPosition(u64);

impl EventPosition {
    pub const ORIGIN: Self = Self(0);

    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Advances the global committed-event position.
    ///
    /// # Errors
    ///
    /// Returns [`SubscriptionIdentityError::EventPositionExhausted`] at
    /// `u64::MAX`.
    pub fn next(self) -> Result<Self, SubscriptionIdentityError> {
        self.0
            .checked_add(1)
            .map(Self)
            .ok_or(SubscriptionIdentityError::EventPositionExhausted)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct EventCursor {
    epoch: ManagerEpoch,
    position: EventPosition,
}

impl EventCursor {
    #[must_use]
    pub const fn new(epoch: ManagerEpoch, position: EventPosition) -> Self {
        Self { epoch, position }
    }

    #[must_use]
    pub const fn epoch(self) -> ManagerEpoch {
        self.epoch
    }

    #[must_use]
    pub const fn position(self) -> EventPosition {
        self.position
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct TopicId(NonZeroU16);

impl TopicId {
    #[must_use]
    pub const fn new(value: NonZeroU16) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u16 {
        self.0.get()
    }
}

impl TryFrom<u16> for TopicId {
    type Error = SubscriptionIdentityError;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        NonZeroU16::new(value)
            .map(Self)
            .ok_or(SubscriptionIdentityError::ZeroTopicId)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TopicFilter {
    All,
    Topics(BTreeSet<TopicId>),
}

impl TopicFilter {
    /// Creates a bounded, nonempty topic selection.
    ///
    /// # Errors
    ///
    /// Returns [`SubscriptionIdentityError`] when the explicit set is empty or
    /// exceeds the protocol bound.
    pub fn topics(topics: BTreeSet<TopicId>) -> Result<Self, SubscriptionIdentityError> {
        if topics.is_empty() {
            return Err(SubscriptionIdentityError::EmptyTopicFilter);
        }
        if topics.len() > MAX_FILTER_TOPICS {
            return Err(SubscriptionIdentityError::TooManyTopics(topics.len()));
        }
        Ok(Self::Topics(topics))
    }

    #[must_use]
    pub fn accepts(&self, topic: TopicId) -> bool {
        match self {
            Self::All => true,
            Self::Topics(topics) => topics.contains(&topic),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct SubscriptionId(NonZeroU64);

impl SubscriptionId {
    #[must_use]
    pub const fn new(value: NonZeroU64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum SubscriptionIdentityError {
    #[error("event position space is exhausted")]
    EventPositionExhausted,
    #[error("topic IDs begin at one")]
    ZeroTopicId,
    #[error("an explicit topic filter must not be empty")]
    EmptyTopicFilter,
    #[error("topic filter has {0} entries; maximum is {MAX_FILTER_TOPICS}")]
    TooManyTopics(usize),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursors_are_epoch_bound_and_positions_fail_closed_at_exhaustion()
    -> Result<(), Box<dyn std::error::Error>> {
        let epoch = ManagerEpoch::new([1; 16])?;
        let cursor = EventCursor::new(epoch, EventPosition::ORIGIN.next()?);
        assert_eq!(cursor.epoch(), epoch);
        assert_eq!(cursor.position().get(), 1);
        assert_eq!(
            EventPosition::new(u64::MAX).next(),
            Err(SubscriptionIdentityError::EventPositionExhausted)
        );
        Ok(())
    }

    #[test]
    fn explicit_filters_are_nonempty_and_select_only_named_topics()
    -> Result<(), SubscriptionIdentityError> {
        assert_eq!(
            TopicFilter::topics(BTreeSet::new()),
            Err(SubscriptionIdentityError::EmptyTopicFilter)
        );
        let window = TopicId::try_from(1)?;
        let workspace = TopicId::try_from(2)?;
        let filter = TopicFilter::topics(BTreeSet::from([window]))?;
        assert!(filter.accepts(window));
        assert!(!filter.accepts(workspace));
        Ok(())
    }
}
