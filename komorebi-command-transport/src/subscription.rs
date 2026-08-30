use std::collections::HashMap;
use std::collections::VecDeque;
use std::num::NonZeroU64;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use komorebi_protocol::AcknowledgedCredit;
use komorebi_protocol::DeliverySequence;
use komorebi_protocol::EventCursor;
use komorebi_protocol::EventPosition;
use komorebi_protocol::FlowError;
use komorebi_protocol::FrameCost;
use komorebi_protocol::LaneLimits;
use komorebi_protocol::ManagerEpoch;
use komorebi_protocol::SubscriptionId;
use komorebi_protocol::SubscriptionIdentityError;
use komorebi_protocol::TopicFilter;
use komorebi_protocol::TopicId;
use thiserror::Error;

use crate::LaneBuildError;
use crate::LanePublisher;
use crate::LaneReceiver;
use crate::SessionMailboxReceivers;
use crate::session_mailbox;

const MIB: u64 = 1024 * 1024;
const REPLAY_BYTES: u64 = 16 * MIB;
const REPLAY_WINDOW: Duration = Duration::from_mins(1);
const LAG_NOTICE_PAYLOAD_BYTES: usize = 32;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SubscriberClass {
    FirstParty,
    Extension,
}

impl SubscriberClass {
    const fn data_limits(self) -> LaneLimits {
        match self {
            Self::FirstParty => LaneLimits::FIRST_PARTY_DATA,
            Self::Extension => LaneLimits::EXTENSION_DATA,
        }
    }
}

#[derive(Debug)]
pub struct EventDelivery<E> {
    cursor: EventCursor,
    topic: TopicId,
    event: Arc<E>,
}

impl<E> Clone for EventDelivery<E> {
    fn clone(&self) -> Self {
        Self {
            cursor: self.cursor,
            topic: self.topic,
            event: Arc::clone(&self.event),
        }
    }
}

impl<E> EventDelivery<E> {
    #[must_use]
    pub const fn cursor(&self) -> EventCursor {
        self.cursor
    }

    #[must_use]
    pub const fn topic(&self) -> TopicId {
        self.topic
    }

    #[must_use]
    pub fn event(&self) -> &E {
        self.event.as_ref()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SubscriptionControl {
    Lagged { through: EventCursor },
}

pub struct SubscriptionStart<S, E> {
    id: SubscriptionId,
    snapshot: Arc<S>,
    cursor: EventCursor,
    data: LaneReceiver<EventDelivery<E>>,
    control: LaneReceiver<SubscriptionControl>,
}

impl<S, E> SubscriptionStart<S, E> {
    #[must_use]
    pub const fn id(&self) -> SubscriptionId {
        self.id
    }

    #[must_use]
    pub fn snapshot(&self) -> &Arc<S> {
        &self.snapshot
    }

    #[must_use]
    pub const fn cursor(&self) -> EventCursor {
        self.cursor
    }

    #[must_use]
    pub fn into_receivers(
        self,
    ) -> (
        LaneReceiver<EventDelivery<E>>,
        LaneReceiver<SubscriptionControl>,
    ) {
        (self.data, self.control)
    }
}

pub struct ResumeStart<E> {
    id: SubscriptionId,
    cursor: EventCursor,
    data: LaneReceiver<EventDelivery<E>>,
    control: LaneReceiver<SubscriptionControl>,
}

impl<E> ResumeStart<E> {
    #[must_use]
    pub const fn id(&self) -> SubscriptionId {
        self.id
    }

    #[must_use]
    pub const fn cursor(&self) -> EventCursor {
        self.cursor
    }

    #[must_use]
    pub fn into_receivers(
        self,
    ) -> (
        LaneReceiver<EventDelivery<E>>,
        LaneReceiver<SubscriptionControl>,
    ) {
        (self.data, self.control)
    }
}

pub enum ResumeDecision<E> {
    Started(ResumeStart<E>),
    ResyncRequired,
}

struct Subscriber<E> {
    filter: TopicFilter,
    data: LanePublisher<EventDelivery<E>>,
    control: LanePublisher<SubscriptionControl>,
}

struct ReplayEntry<E> {
    delivery: EventDelivery<E>,
    cost: FrameCost,
    committed_at: Duration,
}

/// Owns immutable publication snapshots, replay, and per-reader credit.
///
/// The manager's single state owner calls these synchronous methods only after
/// durable commit. No method waits for a reader or invokes manager callbacks.
pub struct EventSubscriptions<S, E> {
    snapshot: Arc<S>,
    cursor: EventCursor,
    started: Instant,
    next_subscription: Option<NonZeroU64>,
    subscribers: HashMap<SubscriptionId, Subscriber<E>>,
    replay: VecDeque<ReplayEntry<E>>,
    replay_bytes: u64,
}

impl<S, E> EventSubscriptions<S, E> {
    #[must_use]
    pub fn new(epoch: ManagerEpoch, snapshot: Arc<S>) -> Self {
        Self {
            snapshot,
            cursor: EventCursor::new(epoch, EventPosition::ORIGIN),
            started: Instant::now(),
            next_subscription: Some(NonZeroU64::MIN),
            subscribers: HashMap::new(),
            replay: VecDeque::new(),
            replay_bytes: 0,
        }
    }

    /// Atomically captures the current immutable snapshot/cursor and registers
    /// the reader's bounded data and reserved control lanes.
    ///
    /// # Errors
    ///
    /// Returns [`SubscriptionError`] if the subscription identity space or
    /// validated lane capacity cannot be represented.
    pub fn subscribe(
        &mut self,
        filter: TopicFilter,
        class: SubscriberClass,
    ) -> Result<SubscriptionStart<S, E>, SubscriptionError> {
        let snapshot = Arc::clone(&self.snapshot);
        let cursor = self.cursor;
        let (id, receivers) = self.register(filter, class)?;
        let (data, control) = split_receivers(receivers);
        Ok(SubscriptionStart {
            id,
            snapshot,
            cursor,
            data,
            control,
        })
    }

    /// Publishes one post-commit event and its matching immutable snapshot.
    ///
    /// Full readers receive a best-effort lag notice on reserved control credit
    /// and are removed. Publication itself never waits.
    ///
    /// # Errors
    ///
    /// Returns [`SubscriptionError`] only when the global event position or
    /// internal byte accounting is exhausted.
    pub fn publish(
        &mut self,
        snapshot: Arc<S>,
        topic: TopicId,
        event: E,
        cost: FrameCost,
    ) -> Result<EventCursor, SubscriptionError> {
        self.publish_at(snapshot, topic, event, cost, self.started.elapsed())
    }

    fn publish_at(
        &mut self,
        snapshot: Arc<S>,
        topic: TopicId,
        event: E,
        cost: FrameCost,
        committed_at: Duration,
    ) -> Result<EventCursor, SubscriptionError> {
        let lag_cost = FrameCost::for_payload(LAG_NOTICE_PAYLOAD_BYTES)?;
        let replay_bytes = self
            .replay_bytes
            .checked_add(u64::from(cost.get()))
            .ok_or(SubscriptionError::ReplayBytesExhausted)?;
        let position = self.cursor.position().next()?;
        let cursor = EventCursor::new(self.cursor.epoch(), position);
        let delivery = EventDelivery {
            cursor,
            topic,
            event: Arc::new(event),
        };
        self.snapshot = snapshot;
        self.cursor = cursor;
        self.replay_bytes = replay_bytes;
        self.replay.push_back(ReplayEntry {
            delivery: delivery.clone(),
            cost,
            committed_at,
        });
        self.trim_replay(committed_at);

        self.subscribers.retain(|_, subscriber| {
            if !subscriber.filter.accepts(topic) {
                return true;
            }
            if subscriber.data.try_publish(cost, delivery.clone()).is_err() {
                let _ = subscriber
                    .control
                    .try_publish(lag_cost, SubscriptionControl::Lagged { through: cursor });
                return false;
            }
            true
        });
        Ok(cursor)
    }

    /// Returns data-lane credit through one delivered sequence.
    ///
    /// # Errors
    ///
    /// Returns [`SubscriptionError::UnknownSubscription`] after lag removal
    /// or a typed flow error for stale and future acknowledgements.
    pub fn acknowledge(
        &mut self,
        id: SubscriptionId,
        through: DeliverySequence,
    ) -> Result<AcknowledgedCredit, SubscriptionError> {
        self.subscribers
            .get_mut(&id)
            .ok_or(SubscriptionError::UnknownSubscription)?
            .data
            .acknowledge(through)
            .map_err(Into::into)
    }

    /// Removes a reader after explicit stream shutdown or connection loss.
    ///
    /// Closing an already absent reader is intentionally idempotent.
    pub fn close(&mut self, id: SubscriptionId) {
        self.subscribers.remove(&id);
    }

    /// Replays retained filtered events after an epoch-bound cursor and then
    /// registers the same live bounded lanes used by a fresh subscription.
    ///
    /// # Errors
    ///
    /// Returns [`SubscriptionError`] if bounded lane construction or
    /// subscription identity allocation fails.
    pub fn resume(
        &mut self,
        cursor: EventCursor,
        filter: TopicFilter,
        class: SubscriberClass,
    ) -> Result<ResumeDecision<E>, SubscriptionError> {
        self.resume_at(cursor, filter, class, self.started.elapsed())
    }

    fn resume_at(
        &mut self,
        cursor: EventCursor,
        filter: TopicFilter,
        class: SubscriberClass,
        now: Duration,
    ) -> Result<ResumeDecision<E>, SubscriptionError> {
        self.trim_replay(now);
        if !self.can_resume(cursor) {
            return Ok(ResumeDecision::ResyncRequired);
        }

        let retained = self
            .replay
            .iter()
            .filter(|entry| {
                entry.delivery.cursor().position() > cursor.position()
                    && filter.accepts(entry.delivery.topic())
            })
            .map(|entry| (entry.delivery.clone(), entry.cost))
            .collect::<Vec<_>>();
        let (id, receivers) = self.register(filter, class)?;
        let Some(subscriber) = self.subscribers.get_mut(&id) else {
            return Err(SubscriptionError::UnknownSubscription);
        };
        if retained
            .into_iter()
            .any(|(delivery, cost)| subscriber.data.try_publish(cost, delivery).is_err())
        {
            self.subscribers.remove(&id);
            return Ok(ResumeDecision::ResyncRequired);
        }
        let (data, control) = split_receivers(receivers);
        Ok(ResumeDecision::Started(ResumeStart {
            id,
            cursor,
            data,
            control,
        }))
    }

    fn can_resume(&self, cursor: EventCursor) -> bool {
        if cursor.epoch() != self.cursor.epoch() || cursor.position() > self.cursor.position() {
            return false;
        }
        if cursor.position() == self.cursor.position() {
            return true;
        }
        self.replay.front().is_some_and(|oldest| {
            cursor
                .position()
                .next()
                .is_ok_and(|next| next >= oldest.delivery.cursor().position())
        })
    }

    /// Invalidates every prior cursor and reader after a manager restart.
    pub fn restart(&mut self, epoch: ManagerEpoch, snapshot: Arc<S>) {
        self.snapshot = snapshot;
        self.cursor = EventCursor::new(epoch, EventPosition::ORIGIN);
        self.started = Instant::now();
        self.subscribers.clear();
        self.replay.clear();
        self.replay_bytes = 0;
    }

    fn register(
        &mut self,
        filter: TopicFilter,
        class: SubscriberClass,
    ) -> Result<Registration<E>, SubscriptionError> {
        let (publishers, receivers) = session_mailbox(class.data_limits())?;
        let id = self.issue_subscription_id()?;
        let (data, control) = publishers.into_parts();
        let subscriber = Subscriber {
            filter,
            data,
            control,
        };
        self.subscribers.insert(id, subscriber);
        Ok((id, receivers))
    }

    fn issue_subscription_id(&mut self) -> Result<SubscriptionId, SubscriptionError> {
        let next = self
            .next_subscription
            .ok_or(SubscriptionError::SubscriptionIdExhausted)?;
        self.next_subscription = NonZeroU64::new(next.get().wrapping_add(1));
        Ok(SubscriptionId::new(next))
    }

    fn trim_replay(&mut self, now: Duration) {
        while self.replay.front().is_some_and(|entry| {
            self.replay_bytes > REPLAY_BYTES
                || now.saturating_sub(entry.committed_at) >= REPLAY_WINDOW
        }) {
            if let Some(removed) = self.replay.pop_front() {
                self.replay_bytes = self
                    .replay_bytes
                    .saturating_sub(u64::from(removed.cost.get()));
            }
        }
    }
}

type Registration<E> = (
    SubscriptionId,
    SessionMailboxReceivers<EventDelivery<E>, SubscriptionControl>,
);

fn split_receivers<E>(
    receivers: SessionMailboxReceivers<EventDelivery<E>, SubscriptionControl>,
) -> (
    LaneReceiver<EventDelivery<E>>,
    LaneReceiver<SubscriptionControl>,
) {
    receivers.into_parts()
}

#[derive(Debug, Error)]
pub enum SubscriptionError {
    #[error("subscription identity space is exhausted")]
    SubscriptionIdExhausted,
    #[error("subscription is unknown or was removed after lag")]
    UnknownSubscription,
    #[error("replay byte accounting overflowed")]
    ReplayBytesExhausted,
    #[error(transparent)]
    Identity(#[from] SubscriptionIdentityError),
    #[error(transparent)]
    Flow(#[from] FlowError),
    #[error(transparent)]
    Lane(#[from] LaneBuildError),
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    fn epoch(value: u8) -> Result<ManagerEpoch, komorebi_protocol::IdentifierError> {
        ManagerEpoch::new([value; 16])
    }

    fn topic(value: u16) -> Result<TopicId, SubscriptionIdentityError> {
        TopicId::try_from(value)
    }

    #[tokio::test(flavor = "current_thread")]
    async fn snapshot_cursor_capture_precedes_the_next_delivery()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut hub = EventSubscriptions::new(epoch(1)?, Arc::new(0_u64));
        hub.publish(Arc::new(1), topic(1)?, "first", FrameCost::for_payload(16)?)?;
        let start = hub.subscribe(TopicFilter::All, SubscriberClass::FirstParty)?;
        assert_eq!(**start.snapshot(), 1);
        assert_eq!(start.cursor().position().get(), 1);
        let id = start.id();
        let (mut data, _) = start.into_receivers();

        hub.publish(
            Arc::new(2),
            topic(2)?,
            "second",
            FrameCost::for_payload(16)?,
        )?;
        let delivery = data.recv().await.ok_or("data lane closed")?;
        assert_eq!(delivery.value().cursor().position().get(), 2);
        assert_eq!(delivery.permit().sequence().get(), 1);
        assert_eq!(
            hub.acknowledge(id, delivery.permit().sequence())?.frames(),
            1
        );
        Ok(())
    }

    #[test]
    fn close_is_idempotent_and_releases_the_reader() -> Result<(), Box<dyn std::error::Error>> {
        let mut subscriptions = EventSubscriptions::<u64, u8>::new(epoch(7)?, Arc::new(0_u64));
        let reader = subscriptions.subscribe(TopicFilter::All, SubscriberClass::FirstParty)?;
        let id = reader.id();

        subscriptions.close(id);
        subscriptions.close(id);

        assert!(matches!(
            subscriptions.acknowledge(id, DeliverySequence::new(NonZeroU64::MIN)),
            Err(SubscriptionError::UnknownSubscription)
        ));
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn extension_byte_exhaustion_uses_reserved_control_and_removes_reader()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut hub = EventSubscriptions::new(epoch(2)?, Arc::new(0_u64));
        let start = hub.subscribe(TopicFilter::All, SubscriberClass::Extension)?;
        let id = start.id();
        let (_stalled_data, mut control) = start.into_receivers();
        let cost = FrameCost::for_payload(700_000)?;
        hub.publish(Arc::new(1), topic(1)?, 1_u8, cost)?;
        let cursor = hub.publish(Arc::new(2), topic(1)?, 2_u8, cost)?;
        let notice = control.recv().await.ok_or("control lane closed")?;
        assert_eq!(
            *notice.value(),
            SubscriptionControl::Lagged { through: cursor }
        );
        assert!(matches!(
            hub.acknowledge(id, DeliverySequence::new(NonZeroU64::MIN)),
            Err(SubscriptionError::UnknownSubscription)
        ));
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn filtered_resume_keeps_delivery_sequence_contiguous()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut hub = EventSubscriptions::new(epoch(3)?, Arc::new(0_u64));
        let origin = EventCursor::new(epoch(3)?, EventPosition::ORIGIN);
        let windows = topic(1)?;
        let workspaces = topic(2)?;
        for (position, event_topic) in [workspaces, windows, workspaces, windows]
            .into_iter()
            .enumerate()
        {
            hub.publish(
                Arc::new(u64::try_from(position + 1)?),
                event_topic,
                u64::try_from(position + 1)?,
                FrameCost::for_payload(16)?,
            )?;
        }
        let filter = TopicFilter::topics(BTreeSet::from([windows]))?;
        let ResumeDecision::Started(start) =
            hub.resume(origin, filter, SubscriberClass::FirstParty)?
        else {
            return Err("retained replay unexpectedly required resync".into());
        };
        let (mut data, _) = start.into_receivers();
        let first = data.recv().await.ok_or("first replay missing")?;
        let second = data.recv().await.ok_or("second replay missing")?;
        assert_eq!(
            (
                first.permit().sequence().get(),
                first.value().cursor().position().get(),
            ),
            (1, 2)
        );
        assert_eq!(
            (
                second.permit().sequence().get(),
                second.value().cursor().position().get(),
            ),
            (2, 4)
        );
        Ok(())
    }

    #[test]
    fn replay_bounds_and_restart_require_resynchronization()
    -> Result<(), Box<dyn std::error::Error>> {
        let original_epoch = epoch(4)?;
        let origin = EventCursor::new(original_epoch, EventPosition::ORIGIN);
        let mut hub = EventSubscriptions::new(original_epoch, Arc::new(0_u64));
        let maximum = FrameCost::for_payload(komorebi_protocol::MAX_FRAME_PAYLOAD_BYTES)?;
        for revision in 1..=17 {
            hub.publish_at(
                Arc::new(revision),
                topic(1)?,
                revision,
                maximum,
                Duration::ZERO,
            )?;
        }
        assert!(matches!(
            hub.resume(origin, TopicFilter::All, SubscriberClass::FirstParty)?,
            ResumeDecision::ResyncRequired
        ));

        hub.restart(epoch(5)?, Arc::new(0));
        assert!(matches!(
            hub.resume(origin, TopicFilter::All, SubscriberClass::FirstParty)?,
            ResumeDecision::ResyncRequired
        ));
        Ok(())
    }

    #[test]
    fn quiet_replay_expiry_requires_resynchronization() -> Result<(), Box<dyn std::error::Error>> {
        let current_epoch = epoch(6)?;
        let origin = EventCursor::new(current_epoch, EventPosition::ORIGIN);
        let mut subscriptions = EventSubscriptions::new(current_epoch, Arc::new(0_u64));
        subscriptions.publish_at(
            Arc::new(1),
            topic(1)?,
            1_u8,
            FrameCost::for_payload(16)?,
            Duration::ZERO,
        )?;

        assert!(matches!(
            subscriptions.resume_at(
                origin,
                TopicFilter::All,
                SubscriberClass::FirstParty,
                REPLAY_WINDOW,
            )?,
            ResumeDecision::ResyncRequired
        ));
        Ok(())
    }
}
