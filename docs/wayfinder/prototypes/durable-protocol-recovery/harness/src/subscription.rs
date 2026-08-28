use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::time::{Duration, Instant};

use serde::Serialize;
use thiserror::Error;

const DATA_FRAMES: usize = 1_024;
const FIRST_PARTY_DATA_BYTES: usize = 4 * 1024 * 1024;
const EXTENSION_DATA_BYTES: usize = 1024 * 1024;
const CONTROL_FRAMES: usize = 64;
const REPLAY_BYTES: usize = 16 * 1024 * 1024;
const REPLAY_WINDOW: Duration = Duration::from_mins(1);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct ManagerEpoch(pub [u8; 16]);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct EventCursor {
    pub epoch: ManagerEpoch,
    pub position: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ManagerSnapshot {
    pub epoch: ManagerEpoch,
    pub revision: u64,
    pub window_count: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Topic {
    Window,
    Workspace,
}

#[derive(Clone, Debug)]
pub struct CommittedEvent {
    pub cursor: EventCursor,
    pub revision: u64,
    pub topic: Topic,
    encoded_bytes: usize,
}

#[derive(Clone, Debug)]
pub struct Delivery {
    pub sequence: u64,
    pub event: CommittedEvent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControlNotice {
    Lagged { through: EventCursor },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Filter {
    All,
    Windows,
}

impl Filter {
    const fn accepts(self, topic: Topic) -> bool {
        matches!(self, Self::All) || matches!(topic, Topic::Window)
    }
}

#[derive(Clone, Copy)]
enum ClientClass {
    FirstParty,
    Extension,
}

impl ClientClass {
    const fn data_bytes(self) -> usize {
        match self {
            Self::FirstParty => FIRST_PARTY_DATA_BYTES,
            Self::Extension => EXTENSION_DATA_BYTES,
        }
    }
}

pub struct SubscriptionStart {
    pub id: u64,
    pub snapshot: Arc<ManagerSnapshot>,
    pub cursor: EventCursor,
    pub data: Receiver<Delivery>,
    pub control: Receiver<ControlNotice>,
}

pub struct ResumeStart {
    pub id: u64,
    pub cursor: EventCursor,
    pub data: Receiver<Delivery>,
    pub control: Receiver<ControlNotice>,
}

struct Subscriber {
    filter: Filter,
    delivery_sequence: u64,
    queued_frames: usize,
    queued_bytes: usize,
    acknowledged_through: u64,
    pending: VecDeque<(u64, usize)>,
    maximum_bytes: usize,
    data: SyncSender<Delivery>,
    control: SyncSender<ControlNotice>,
}

struct ReplayEntry {
    event: CommittedEvent,
    committed_at: Duration,
}

pub struct StateOwner {
    snapshot: Arc<ManagerSnapshot>,
    started: Instant,
    next_subscription: u64,
    subscribers: HashMap<u64, Subscriber>,
    replay: VecDeque<ReplayEntry>,
    replay_bytes: usize,
}

impl StateOwner {
    pub fn new(epoch: ManagerEpoch) -> Self {
        Self {
            snapshot: Arc::new(ManagerSnapshot {
                epoch,
                revision: 0,
                window_count: 0,
            }),
            started: Instant::now(),
            next_subscription: 1,
            subscribers: HashMap::new(),
            replay: VecDeque::new(),
            replay_bytes: 0,
        }
    }

    pub fn subscribe(&mut self, filter: Filter) -> SubscriptionStart {
        self.subscribe_as(filter, ClientClass::FirstParty)
    }

    pub fn subscribe_extension(&mut self, filter: Filter) -> SubscriptionStart {
        self.subscribe_as(filter, ClientClass::Extension)
    }

    fn subscribe_as(&mut self, filter: Filter, class: ClientClass) -> SubscriptionStart {
        let snapshot = Arc::clone(&self.snapshot);
        let cursor = EventCursor {
            epoch: snapshot.epoch,
            position: snapshot.revision,
        };
        let (id, data, control) = self.register(filter, class);
        SubscriptionStart {
            id,
            snapshot,
            cursor,
            data,
            control,
        }
    }

    fn register(
        &mut self,
        filter: Filter,
        class: ClientClass,
    ) -> (u64, Receiver<Delivery>, Receiver<ControlNotice>) {
        let (data, data_rx) = sync_channel(DATA_FRAMES);
        let (control, control_rx) = sync_channel(CONTROL_FRAMES);
        let id = self.next_subscription;
        self.next_subscription = self.next_subscription.saturating_add(1);
        self.subscribers.insert(
            id,
            Subscriber {
                filter,
                delivery_sequence: 0,
                queued_frames: 0,
                queued_bytes: 0,
                acknowledged_through: 0,
                pending: VecDeque::new(),
                maximum_bytes: class.data_bytes(),
                data,
                control,
            },
        );
        (id, data_rx, control_rx)
    }

    pub fn publish(&mut self, topic: Topic) -> EventCursor {
        self.publish_at(topic, 64, self.started.elapsed())
    }

    fn publish_at(
        &mut self,
        topic: Topic,
        encoded_bytes: usize,
        committed_at: Duration,
    ) -> EventCursor {
        let revision = self.snapshot.revision.saturating_add(1);
        let cursor = EventCursor {
            epoch: self.snapshot.epoch,
            position: revision,
        };
        self.snapshot = Arc::new(ManagerSnapshot {
            epoch: self.snapshot.epoch,
            revision,
            window_count: self
                .snapshot
                .window_count
                .saturating_add(u32::from(matches!(topic, Topic::Window))),
        });
        let event = CommittedEvent {
            cursor,
            revision,
            topic,
            encoded_bytes,
        };
        self.replay_bytes = self.replay_bytes.saturating_add(encoded_bytes);
        self.replay.push_back(ReplayEntry {
            event: event.clone(),
            committed_at,
        });
        self.trim_replay(committed_at);

        let mut remove = Vec::new();
        for (id, subscriber) in &mut self.subscribers {
            if !subscriber.filter.accepts(topic) {
                continue;
            }
            let exceeds_frames = subscriber.queued_frames >= DATA_FRAMES;
            let exceeds_bytes = subscriber
                .queued_bytes
                .checked_add(encoded_bytes)
                .is_none_or(|bytes| bytes > subscriber.maximum_bytes);
            if exceeds_frames || exceeds_bytes {
                signal_lag(subscriber, cursor);
                remove.push(*id);
                continue;
            }
            let sequence = subscriber.delivery_sequence.saturating_add(1);
            match subscriber.data.try_send(Delivery {
                sequence,
                event: event.clone(),
            }) {
                Ok(()) => {
                    subscriber.delivery_sequence = sequence;
                    subscriber.queued_frames = subscriber.queued_frames.saturating_add(1);
                    subscriber.queued_bytes = subscriber.queued_bytes.saturating_add(encoded_bytes);
                    subscriber.pending.push_back((sequence, encoded_bytes));
                }
                Err(TrySendError::Full(_)) => {
                    signal_lag(subscriber, cursor);
                    remove.push(*id);
                }
                Err(TrySendError::Disconnected(_)) => remove.push(*id),
            }
        }
        for id in remove {
            self.subscribers.remove(&id);
        }
        cursor
    }

    fn trim_replay(&mut self, now: Duration) {
        while self.replay.front().is_some_and(|entry| {
            self.replay_bytes > REPLAY_BYTES
                || now.saturating_sub(entry.committed_at) > REPLAY_WINDOW
        }) {
            if let Some(removed) = self.replay.pop_front() {
                self.replay_bytes = self
                    .replay_bytes
                    .saturating_sub(removed.event.encoded_bytes);
            }
        }
    }

    pub fn acknowledge(
        &mut self,
        subscription_id: u64,
        through: u64,
    ) -> Result<(), AcknowledgeError> {
        let subscriber = self
            .subscribers
            .get_mut(&subscription_id)
            .ok_or(AcknowledgeError::UnknownSubscription)?;
        if through <= subscriber.acknowledged_through || through > subscriber.delivery_sequence {
            return Err(AcknowledgeError::InvalidSequence {
                acknowledged_through: subscriber.acknowledged_through,
                delivered_through: subscriber.delivery_sequence,
                requested: through,
            });
        }
        while subscriber
            .pending
            .front()
            .is_some_and(|(sequence, _)| *sequence <= through)
        {
            if let Some((_, bytes)) = subscriber.pending.pop_front() {
                subscriber.queued_frames = subscriber.queued_frames.saturating_sub(1);
                subscriber.queued_bytes = subscriber.queued_bytes.saturating_sub(bytes);
            }
        }
        subscriber.acknowledged_through = through;
        Ok(())
    }

    pub fn resume(&mut self, cursor: EventCursor, filter: Filter) -> Resume {
        if cursor.epoch != self.snapshot.epoch {
            return Resume::ResyncRequired;
        }
        let oldest = self
            .replay
            .front()
            .map_or(self.snapshot.revision, |entry| entry.event.cursor.position);
        if cursor.position.saturating_add(1) < oldest {
            return Resume::ResyncRequired;
        }
        let replay = self
            .replay
            .iter()
            .filter(|entry| {
                entry.event.cursor.position > cursor.position && filter.accepts(entry.event.topic)
            })
            .map(|entry| entry.event.clone())
            .collect::<Vec<_>>();
        let replay_bytes = replay.iter().try_fold(0_usize, |total, event| {
            total.checked_add(event.encoded_bytes)
        });
        if replay.len() > DATA_FRAMES
            || replay_bytes.is_none_or(|bytes| bytes > FIRST_PARTY_DATA_BYTES)
        {
            return Resume::ResyncRequired;
        }
        let (id, data, control) = self.register(filter, ClientClass::FirstParty);
        let Some(subscriber) = self.subscribers.get_mut(&id) else {
            return Resume::ResyncRequired;
        };
        for event in replay {
            let sequence = subscriber.delivery_sequence.saturating_add(1);
            if subscriber
                .data
                .try_send(Delivery {
                    sequence,
                    event: event.clone(),
                })
                .is_err()
            {
                self.subscribers.remove(&id);
                return Resume::ResyncRequired;
            }
            subscriber.delivery_sequence = sequence;
            subscriber.queued_frames = subscriber.queued_frames.saturating_add(1);
            subscriber.queued_bytes = subscriber.queued_bytes.saturating_add(event.encoded_bytes);
            subscriber
                .pending
                .push_back((sequence, event.encoded_bytes));
        }
        Resume::Started(ResumeStart {
            id,
            cursor,
            data,
            control,
        })
    }

    pub fn restart(&mut self, epoch: ManagerEpoch) {
        self.snapshot = Arc::new(ManagerSnapshot {
            epoch,
            revision: 0,
            window_count: 0,
        });
        self.started = Instant::now();
        self.subscribers.clear();
        self.replay.clear();
        self.replay_bytes = 0;
    }
}

fn signal_lag(subscriber: &Subscriber, through: EventCursor) {
    match subscriber
        .control
        .try_send(ControlNotice::Lagged { through })
    {
        Ok(()) | Err(TrySendError::Disconnected(_) | TrySendError::Full(_)) => {
            // The subscriber is removed immediately after this call. A full reserved control
            // lane already contains a terminal lag notice; a disconnected lane has no reader.
        }
    }
}

pub enum Resume {
    Started(ResumeStart),
    ResyncRequired,
}

#[derive(Debug, Error)]
pub enum AcknowledgeError {
    #[error("subscription no longer exists")]
    UnknownSubscription,
    #[error("acknowledgement {requested} is outside ({acknowledged_through}, {delivered_through}]")]
    InvalidSequence {
        acknowledged_through: u64,
        delivered_through: u64,
        requested: u64,
    },
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use proptest::prelude::*;

    use super::{ControlNotice, Filter, ManagerEpoch, Resume, StateOwner, Topic};

    proptest! {
        #[test]
        fn filtered_publication_preserves_global_and_delivery_order(
            window_topics in prop::collection::vec(any::<bool>(), 0..512),
        ) {
            let mut owner = StateOwner::new(ManagerEpoch([10; 16]));
            let start = owner.subscribe(Filter::Windows);
            let mut accepted_positions = Vec::new();
            for (offset, is_window) in window_topics.iter().copied().enumerate() {
                let topic = if is_window { Topic::Window } else { Topic::Workspace };
                owner.publish(topic);
                if is_window {
                    accepted_positions.push(
                        u64::try_from(offset + 1)
                            .map_err(|error| TestCaseError::fail(error.to_string()))?,
                    );
                }
            }

            let deliveries = start.data.try_iter().collect::<Vec<_>>();
            prop_assert_eq!(deliveries.len(), accepted_positions.len());
            for (offset, (delivery, expected_position)) in deliveries
                .iter()
                .zip(accepted_positions)
                .enumerate()
            {
                prop_assert_eq!(
                    delivery.sequence,
                    u64::try_from(offset + 1)
                        .map_err(|error| TestCaseError::fail(error.to_string()))?
                );
                prop_assert_eq!(delivery.event.cursor.position, expected_position);
                prop_assert_eq!(delivery.event.revision, expected_position);
            }
        }
    }

    #[test]
    fn subscription_start_is_atomic_with_snapshot_cursor() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut owner = StateOwner::new(ManagerEpoch([1; 16]));
        owner.publish(Topic::Window);
        let start = owner.subscribe(Filter::All);
        assert_eq!(start.snapshot.revision, start.cursor.position);
        owner.publish(Topic::Workspace);
        let delivery = start.data.recv()?;
        assert_eq!(delivery.event.cursor.position, start.cursor.position + 1);
        Ok(())
    }

    #[test]
    fn filtered_resume_skips_global_positions_but_keeps_delivery_contiguous()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut owner = StateOwner::new(ManagerEpoch([2; 16]));
        let cursor = owner.subscribe(Filter::All).cursor;
        owner.publish(Topic::Workspace);
        owner.publish(Topic::Window);
        owner.publish(Topic::Workspace);
        owner.publish(Topic::Window);
        let Resume::Started(resume) = owner.resume(cursor, Filter::Windows) else {
            return Err("resume unexpectedly required a snapshot".into());
        };
        let first = resume.data.recv()?;
        let second = resume.data.recv()?;
        assert_eq!((first.sequence, first.event.cursor.position), (1, 2));
        assert_eq!((second.sequence, second.event.cursor.position), (2, 4));
        Ok(())
    }

    #[test]
    fn first_party_frame_limit_uses_reserved_control_lane() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut owner = StateOwner::new(ManagerEpoch([3; 16]));
        let start = owner.subscribe(Filter::All);
        for _ in 0..=1_024 {
            owner.publish(Topic::Window);
        }
        assert!(matches!(
            start.control.recv()?,
            ControlNotice::Lagged { .. }
        ));
        Ok(())
    }

    #[test]
    fn extension_byte_limit_uses_reserved_control_lane() -> Result<(), Box<dyn std::error::Error>> {
        let mut owner = StateOwner::new(ManagerEpoch([4; 16]));
        let start = owner.subscribe_extension(Filter::All);
        owner.publish_at(Topic::Window, 700_000, Duration::ZERO);
        owner.publish_at(Topic::Window, 700_000, Duration::ZERO);
        assert!(matches!(
            start.control.recv()?,
            ControlNotice::Lagged { .. }
        ));
        Ok(())
    }

    #[test]
    fn acknowledgement_returns_credit_without_changing_delivery_order()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut owner = StateOwner::new(ManagerEpoch([9; 16]));
        let start = owner.subscribe(Filter::All);
        for _ in 0..1_024 {
            owner.publish(Topic::Window);
        }
        let mut through = 0;
        for _ in 0..1_024 {
            through = start.data.recv()?.sequence;
        }
        owner.acknowledge(start.id, through)?;
        owner.publish(Topic::Window);
        assert_eq!(start.data.recv()?.sequence, 1_025);
        assert!(owner.acknowledge(start.id, through).is_err());
        Ok(())
    }

    #[test]
    fn replay_ring_expires_by_byte_and_time_boundaries() {
        let mut owner = StateOwner::new(ManagerEpoch([5; 16]));
        let cursor = owner.subscribe(Filter::All).cursor;
        owner.publish_at(Topic::Window, 10 * 1024 * 1024, Duration::ZERO);
        owner.publish_at(Topic::Window, 10 * 1024 * 1024, Duration::ZERO);
        assert!(matches!(
            owner.resume(cursor, Filter::All),
            Resume::ResyncRequired
        ));

        let mut owner = StateOwner::new(ManagerEpoch([6; 16]));
        let cursor = owner.subscribe(Filter::All).cursor;
        owner.publish_at(Topic::Window, 64, Duration::ZERO);
        owner.publish_at(Topic::Window, 64, Duration::from_secs(61));
        assert!(matches!(
            owner.resume(cursor, Filter::All),
            Resume::ResyncRequired
        ));
    }

    #[test]
    fn restart_requires_resnapshot() {
        let mut owner = StateOwner::new(ManagerEpoch([7; 16]));
        let cursor = owner.subscribe(Filter::All).cursor;
        owner.restart(ManagerEpoch([8; 16]));
        assert!(matches!(
            owner.resume(cursor, Filter::All),
            Resume::ResyncRequired
        ));
    }
}
