use std::fmt;

use komorebi_protocol::AcknowledgedCredit;
use komorebi_protocol::DeliveryPermit;
use komorebi_protocol::DeliverySequence;
use komorebi_protocol::FlowController;
use komorebi_protocol::FlowError;
use komorebi_protocol::FrameCost;
use komorebi_protocol::LaneLimits;
use thiserror::Error;
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::TrySendError;

#[derive(Debug)]
pub struct LaneMessage<T> {
    permit: DeliveryPermit,
    value: T,
}

impl<T> LaneMessage<T> {
    #[must_use]
    pub const fn permit(&self) -> DeliveryPermit {
        self.permit
    }

    #[must_use]
    pub fn value(&self) -> &T {
        &self.value
    }

    #[must_use]
    pub fn into_parts(self) -> (DeliveryPermit, T) {
        (self.permit, self.value)
    }
}

pub struct LanePublisher<T> {
    sender: mpsc::Sender<LaneMessage<T>>,
    flow: FlowController,
}

impl<T> LanePublisher<T> {
    /// Attempts immediate publication without waiting for channel or byte credit.
    ///
    /// # Errors
    ///
    /// Returns the unsent value and a typed channel or flow-control reason.
    pub fn try_publish(
        &mut self,
        cost: FrameCost,
        value: T,
    ) -> Result<DeliverySequence, LanePublishError<T>> {
        let Self { sender, flow } = self;
        let channel_permit = match sender.try_reserve() {
            Ok(permit) => permit,
            Err(TrySendError::Full(())) => {
                return Err(LanePublishError {
                    reason: LanePublishFailure::ChannelFull,
                    value,
                });
            }
            Err(TrySendError::Closed(())) => {
                return Err(LanePublishError {
                    reason: LanePublishFailure::ChannelClosed,
                    value,
                });
            }
        };
        let delivery = match flow.reserve(cost) {
            Ok(delivery) => delivery,
            Err(error) => {
                return Err(LanePublishError {
                    reason: LanePublishFailure::Flow(error),
                    value,
                });
            }
        };
        let sequence = delivery.sequence();
        channel_permit.send(LaneMessage {
            permit: delivery,
            value,
        });
        Ok(sequence)
    }

    /// Returns cumulative credit through an acknowledged delivery.
    ///
    /// # Errors
    ///
    /// Returns [`FlowError`] for a duplicate or future acknowledgement.
    pub fn acknowledge(
        &mut self,
        sequence: DeliverySequence,
    ) -> Result<AcknowledgedCredit, FlowError> {
        self.flow.acknowledge(sequence)
    }

    #[must_use]
    pub fn outstanding_frames(&self) -> usize {
        self.flow.outstanding_frames()
    }

    #[must_use]
    pub const fn outstanding_bytes(&self) -> u32 {
        self.flow.outstanding_bytes()
    }

    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.sender.is_closed()
    }
}

pub struct LaneReceiver<T> {
    receiver: mpsc::Receiver<LaneMessage<T>>,
}

impl<T> LaneReceiver<T> {
    /// Waits for one publication or channel closure.
    ///
    /// Cancellation is safe because Tokio's bounded `recv` does not consume a
    /// value unless it returns that value.
    pub async fn recv(&mut self) -> Option<LaneMessage<T>> {
        self.receiver.recv().await
    }

    pub fn close(&mut self) {
        self.receiver.close();
    }
}

/// Creates one nonblocking publisher and one async receiver with shared limits.
///
/// # Errors
///
/// Returns [`LaneBuildError`] if the validated frame capacity cannot fit the
/// target address space.
pub fn bounded_lane<T>(
    limits: LaneLimits,
) -> Result<(LanePublisher<T>, LaneReceiver<T>), LaneBuildError> {
    let capacity = usize::try_from(limits.max_frames())
        .map_err(|_| LaneBuildError::CapacityOutsideAddressSpace(limits.max_frames()))?;
    let (sender, receiver) = mpsc::channel(capacity);
    Ok((
        LanePublisher {
            sender,
            flow: FlowController::new(limits),
        },
        LaneReceiver { receiver },
    ))
}

pub struct SessionMailboxPublishers<D, C> {
    data: LanePublisher<D>,
    control: LanePublisher<C>,
}

impl<D, C> SessionMailboxPublishers<D, C> {
    pub const fn data_mut(&mut self) -> &mut LanePublisher<D> {
        &mut self.data
    }

    pub const fn control_mut(&mut self) -> &mut LanePublisher<C> {
        &mut self.control
    }

    #[must_use]
    pub fn into_parts(self) -> (LanePublisher<D>, LanePublisher<C>) {
        (self.data, self.control)
    }
}

pub struct SessionMailboxReceivers<D, C> {
    data: LaneReceiver<D>,
    control: LaneReceiver<C>,
}

pub type SessionMailbox<D, C> = (
    SessionMailboxPublishers<D, C>,
    SessionMailboxReceivers<D, C>,
);

impl<D, C> SessionMailboxReceivers<D, C> {
    pub const fn data_mut(&mut self) -> &mut LaneReceiver<D> {
        &mut self.data
    }

    pub const fn control_mut(&mut self) -> &mut LaneReceiver<C> {
        &mut self.control
    }

    #[must_use]
    pub fn into_parts(self) -> (LaneReceiver<D>, LaneReceiver<C>) {
        (self.data, self.control)
    }
}

/// Creates independent data and reserved control lanes.
///
/// # Errors
///
/// Returns [`LaneBuildError`] if a validated capacity does not fit the address
/// space.
pub fn session_mailbox<D, C>(
    data_limits: LaneLimits,
) -> Result<SessionMailbox<D, C>, LaneBuildError> {
    let (data, data_receiver) = bounded_lane(data_limits)?;
    let (control, control_receiver) = bounded_lane(LaneLimits::CONTROL)?;
    Ok((
        SessionMailboxPublishers { data, control },
        SessionMailboxReceivers {
            data: data_receiver,
            control: control_receiver,
        },
    ))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LanePublishFailure {
    ChannelFull,
    ChannelClosed,
    Flow(FlowError),
}

pub struct LanePublishError<T> {
    reason: LanePublishFailure,
    value: T,
}

impl<T> LanePublishError<T> {
    #[must_use]
    pub const fn reason(&self) -> LanePublishFailure {
        self.reason
    }

    #[must_use]
    pub fn into_value(self) -> T {
        self.value
    }
}

impl<T> fmt::Debug for LanePublishError<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LanePublishError")
            .field("reason", &self.reason)
            .finish_non_exhaustive()
    }
}

impl<T> fmt::Display for LanePublishError<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "lane publication rejected: {}", self.reason)
    }
}

impl<T> std::error::Error for LanePublishError<T> {}

impl fmt::Display for LanePublishFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ChannelFull => formatter.write_str("bounded channel is full"),
            Self::ChannelClosed => formatter.write_str("bounded channel is closed"),
            Self::Flow(error) => error.fmt(formatter),
        }
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum LaneBuildError {
    #[error("lane frame capacity {0} is outside the target address space")]
    CapacityOutsideAddressSpace(u32),
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use super::*;

    #[derive(Debug, Error)]
    enum TestError {
        #[error("publication unexpectedly succeeded")]
        UnexpectedPublish,
        #[error("lane unexpectedly closed")]
        UnexpectedClose,
    }

    fn limits(frames: u32, bytes: u32) -> Result<LaneLimits, FlowError> {
        LaneLimits::new(
            NonZeroU32::new(frames).ok_or(FlowError::LaneFrameLimitTooLarge(frames))?,
            NonZeroU32::new(bytes).ok_or(FlowError::LaneByteLimitTooLarge(bytes))?,
        )
    }

    #[tokio::test(flavor = "current_thread")]
    async fn receiving_does_not_restore_credit_before_acknowledgement()
    -> Result<(), Box<dyn std::error::Error>> {
        let cost = FrameCost::for_payload(8)?;
        let (mut publisher, mut receiver) = bounded_lane(limits(2, 1024)?)?;
        let first = publisher.try_publish(cost, "first")?;
        publisher.try_publish(cost, "second")?;
        let Err(rejected) = publisher.try_publish(cost, "third") else {
            return Err(TestError::UnexpectedPublish.into());
        };
        assert_eq!(rejected.reason(), LanePublishFailure::ChannelFull);

        let message = receiver.recv().await.ok_or(TestError::UnexpectedClose)?;
        assert_eq!(message.value(), &"first");
        let Err(rejected) = publisher.try_publish(cost, rejected.into_value()) else {
            return Err(TestError::UnexpectedPublish.into());
        };
        assert_eq!(
            rejected.reason(),
            LanePublishFailure::Flow(FlowError::FrameCreditExhausted)
        );

        let acknowledged = publisher.acknowledge(first)?;
        assert_eq!(acknowledged.frames(), 1);
        publisher.try_publish(cost, rejected.into_value())?;
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn control_lane_remains_available_when_data_lane_is_full()
    -> Result<(), Box<dyn std::error::Error>> {
        let (mut publishers, mut receivers) = session_mailbox::<u8, u8>(limits(1, 1024)?)?;
        let cost = FrameCost::for_payload(1)?;
        publishers.data_mut().try_publish(cost, 1)?;
        assert!(publishers.data_mut().try_publish(cost, 2).is_err());
        publishers.control_mut().try_publish(cost, 9)?;
        assert_eq!(
            receivers
                .control_mut()
                .recv()
                .await
                .ok_or(TestError::UnexpectedClose)?
                .value(),
            &9
        );
        Ok(())
    }
}
