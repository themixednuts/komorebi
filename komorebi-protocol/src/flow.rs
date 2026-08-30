use std::collections::VecDeque;
use std::num::NonZeroU32;
use std::num::NonZeroU64;

use thiserror::Error;

use crate::HEADER_BYTES;
use crate::MAX_FRAME_PAYLOAD_BYTES;
use crate::frame::HEADER_WIRE_BYTES;

const KIB: u32 = 1024;
const MIB: u32 = KIB * KIB;
const MAX_LANE_FRAMES: u32 = 65_536;
const MAX_LANE_BYTES: u32 = 8 * MIB;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct DeliverySequence(NonZeroU64);

impl DeliverySequence {
    #[must_use]
    pub const fn new(value: NonZeroU64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameCost(NonZeroU32);

impl FrameCost {
    /// Calculates the complete wire-frame byte cost, including its header.
    ///
    /// # Errors
    ///
    /// Returns [`FlowError::FramePayloadTooLarge`] above the framing ceiling.
    pub fn for_payload(payload_bytes: usize) -> Result<Self, FlowError> {
        if payload_bytes > MAX_FRAME_PAYLOAD_BYTES {
            return Err(FlowError::FramePayloadTooLarge(payload_bytes));
        }
        let wire_bytes = payload_bytes
            .checked_add(HEADER_BYTES)
            .and_then(|value| u32::try_from(value).ok())
            .ok_or(FlowError::FramePayloadTooLarge(payload_bytes))?;
        NonZeroU32::new(wire_bytes)
            .map(Self)
            .ok_or(FlowError::FramePayloadTooLarge(payload_bytes))
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0.get()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LaneLimits {
    max_frames: u32,
    max_bytes: u32,
}

impl LaneLimits {
    pub const FIRST_PARTY_DATA: Self = Self {
        max_frames: 1024,
        max_bytes: 4 * MIB,
    };
    pub const EXTENSION_DATA: Self = Self {
        max_frames: MIB / HEADER_WIRE_BYTES,
        max_bytes: MIB,
    };
    pub const CONTROL: Self = Self {
        max_frames: 64,
        max_bytes: 256 * KIB,
    };

    /// Creates bounded frame and byte limits for one lane.
    ///
    /// # Errors
    ///
    /// Returns [`FlowError`] when a limit exceeds the v1 absolute bounds.
    pub fn new(max_frames: NonZeroU32, max_bytes: NonZeroU32) -> Result<Self, FlowError> {
        if max_frames.get() > MAX_LANE_FRAMES {
            return Err(FlowError::LaneFrameLimitTooLarge(max_frames.get()));
        }
        if max_bytes.get() > MAX_LANE_BYTES {
            return Err(FlowError::LaneByteLimitTooLarge(max_bytes.get()));
        }
        Ok(Self {
            max_frames: max_frames.get(),
            max_bytes: max_bytes.get(),
        })
    }

    #[must_use]
    pub const fn max_frames(self) -> u32 {
        self.max_frames
    }

    #[must_use]
    pub const fn max_bytes(self) -> u32 {
        self.max_bytes
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeliveryPermit {
    sequence: DeliverySequence,
    cost: FrameCost,
}

impl DeliveryPermit {
    #[must_use]
    pub const fn sequence(self) -> DeliverySequence {
        self.sequence
    }

    #[must_use]
    pub const fn cost(self) -> FrameCost {
        self.cost
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AcknowledgedCredit {
    frames: u32,
    bytes: u32,
}

impl AcknowledgedCredit {
    #[must_use]
    pub const fn frames(self) -> u32 {
        self.frames
    }

    #[must_use]
    pub const fn bytes(self) -> u32 {
        self.bytes
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FlowController {
    limits: LaneLimits,
    outstanding: VecDeque<DeliveryPermit>,
    used_bytes: u32,
    next_sequence: Option<NonZeroU64>,
    last_acknowledged: Option<DeliverySequence>,
}

impl FlowController {
    #[must_use]
    pub fn new(limits: LaneLimits) -> Self {
        Self {
            limits,
            outstanding: VecDeque::new(),
            used_bytes: 0,
            next_sequence: Some(NonZeroU64::MIN),
            last_acknowledged: None,
        }
    }

    /// Reserves one frame and its full wire-byte cost.
    ///
    /// # Errors
    ///
    /// Returns a capacity, oversized-frame, or sequence-exhaustion error without
    /// changing the account.
    pub fn reserve(&mut self, cost: FrameCost) -> Result<DeliveryPermit, FlowError> {
        if cost.get() > self.limits.max_bytes {
            return Err(FlowError::FrameExceedsLane {
                frame_bytes: cost.get(),
                lane_bytes: self.limits.max_bytes,
            });
        }
        let used_frames =
            u32::try_from(self.outstanding.len()).map_err(|_| FlowError::FrameCreditExhausted)?;
        if used_frames >= self.limits.max_frames {
            return Err(FlowError::FrameCreditExhausted);
        }
        let next_used_bytes = self
            .used_bytes
            .checked_add(cost.get())
            .ok_or(FlowError::ByteCreditExhausted)?;
        if next_used_bytes > self.limits.max_bytes {
            return Err(FlowError::ByteCreditExhausted);
        }
        let next = self.next_sequence.ok_or(FlowError::SequenceExhausted)?;
        let permit = DeliveryPermit {
            sequence: DeliverySequence::new(next),
            cost,
        };
        self.next_sequence = NonZeroU64::new(next.get().wrapping_add(1));
        self.used_bytes = next_used_bytes;
        self.outstanding.push_back(permit);
        Ok(permit)
    }

    /// Returns cumulative frame and byte credit through `sequence`.
    ///
    /// # Errors
    ///
    /// Returns [`FlowError::DuplicateAcknowledgement`] for an old ACK and
    /// [`FlowError::FutureAcknowledgement`] for a sequence never issued.
    pub fn acknowledge(
        &mut self,
        sequence: DeliverySequence,
    ) -> Result<AcknowledgedCredit, FlowError> {
        if self.last_acknowledged.is_some_and(|last| sequence <= last) {
            return Err(FlowError::DuplicateAcknowledgement(sequence));
        }
        let Some(position) = self
            .outstanding
            .iter()
            .position(|permit| permit.sequence == sequence)
        else {
            return Err(FlowError::FutureAcknowledgement(sequence));
        };
        let frames = u32::try_from(position + 1).map_err(|_| FlowError::FrameCreditExhausted)?;
        let bytes = self
            .outstanding
            .drain(..=position)
            .map(|permit| permit.cost.get())
            .sum();
        self.used_bytes -= bytes;
        self.last_acknowledged = Some(sequence);
        Ok(AcknowledgedCredit { frames, bytes })
    }

    #[must_use]
    pub fn outstanding_frames(&self) -> usize {
        self.outstanding.len()
    }

    #[must_use]
    pub const fn outstanding_bytes(&self) -> u32 {
        self.used_bytes
    }

    #[must_use]
    pub const fn limits(&self) -> LaneLimits {
        self.limits
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum FlowError {
    #[error("frame payload is {0} bytes; above the protocol framing ceiling")]
    FramePayloadTooLarge(usize),
    #[error("lane frame limit {0} exceeds the v1 maximum")]
    LaneFrameLimitTooLarge(u32),
    #[error("lane byte limit {0} exceeds the v1 maximum")]
    LaneByteLimitTooLarge(u32),
    #[error("frame costs {frame_bytes} bytes but lane ceiling is {lane_bytes}")]
    FrameExceedsLane { frame_bytes: u32, lane_bytes: u32 },
    #[error("lane frame credit is exhausted")]
    FrameCreditExhausted,
    #[error("lane byte credit is exhausted")]
    ByteCreditExhausted,
    #[error("delivery sequence is exhausted")]
    SequenceExhausted,
    #[error("delivery acknowledgement {0:?} is duplicate or stale")]
    DuplicateAcknowledgement(DeliverySequence),
    #[error("delivery acknowledgement {0:?} has not been issued")]
    FutureAcknowledgement(DeliverySequence),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits(frames: u32, bytes: u32) -> Result<LaneLimits, FlowError> {
        LaneLimits::new(
            NonZeroU32::new(frames).ok_or(FlowError::LaneFrameLimitTooLarge(frames))?,
            NonZeroU32::new(bytes).ok_or(FlowError::LaneByteLimitTooLarge(bytes))?,
        )
    }

    fn delivery(value: u64) -> Result<DeliverySequence, FlowError> {
        NonZeroU64::new(value)
            .map(DeliverySequence::new)
            .ok_or(FlowError::SequenceExhausted)
    }

    #[test]
    fn full_wire_cost_makes_zero_payload_traffic_finite() -> Result<(), FlowError> {
        assert_eq!(FrameCost::for_payload(0)?.get(), HEADER_WIRE_BYTES);
        assert_eq!(
            LaneLimits::EXTENSION_DATA.max_frames(),
            LaneLimits::EXTENSION_DATA.max_bytes() / HEADER_WIRE_BYTES
        );
        Ok(())
    }

    #[test]
    fn frame_and_byte_credit_are_independently_bounded() -> Result<(), FlowError> {
        let cost = FrameCost::for_payload(8)?;
        let mut frame_limited = FlowController::new(limits(2, 1024)?);
        frame_limited.reserve(cost)?;
        frame_limited.reserve(cost)?;
        assert_eq!(
            frame_limited.reserve(cost),
            Err(FlowError::FrameCreditExhausted)
        );

        let mut byte_limited = FlowController::new(limits(10, cost.get() * 2)?);
        byte_limited.reserve(cost)?;
        byte_limited.reserve(cost)?;
        assert_eq!(
            byte_limited.reserve(cost),
            Err(FlowError::ByteCreditExhausted)
        );
        Ok(())
    }

    #[test]
    fn acknowledgements_are_cumulative_monotonic_and_bounded() -> Result<(), FlowError> {
        let cost = FrameCost::for_payload(8)?;
        let mut flow = FlowController::new(limits(4, 1024)?);
        flow.reserve(cost)?;
        flow.reserve(cost)?;
        flow.reserve(cost)?;

        assert_eq!(
            flow.acknowledge(delivery(4)?),
            Err(FlowError::FutureAcknowledgement(delivery(4)?))
        );
        assert_eq!(
            flow.acknowledge(delivery(2)?)?,
            AcknowledgedCredit {
                frames: 2,
                bytes: cost.get() * 2,
            }
        );
        assert_eq!(flow.outstanding_frames(), 1);
        assert_eq!(flow.outstanding_bytes(), cost.get());
        assert_eq!(
            flow.acknowledge(delivery(2)?),
            Err(FlowError::DuplicateAcknowledgement(delivery(2)?))
        );
        Ok(())
    }
}
