use std::num::NonZeroU32;

use crate::FrameError;

const PREFACE_BYTES: [u8; 8] = [b'K', b'C', b'M', b'D', 0, 1, 0, 0];

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ProtocolPreface;

impl ProtocolPreface {
    #[must_use]
    pub const fn encode(self) -> [u8; PREFACE_BYTES.len()] {
        PREFACE_BYTES
    }

    /// Validates the fixed version 1 protocol preface.
    ///
    /// # Errors
    ///
    /// Returns [`FrameError::InvalidPreface`] when `bytes` is not the exact
    /// eight-byte version 1 preface.
    pub fn decode(bytes: &[u8]) -> Result<Self, FrameError> {
        if bytes == PREFACE_BYTES {
            Ok(Self)
        } else {
            Err(FrameError::InvalidPreface)
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct FrameKind(u16);

impl FrameKind {
    #[must_use]
    pub const fn new(value: u16) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct DirectionSequence(u64);

impl DirectionSequence {
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum StreamId {
    Control,
    ClientInitiated(NonZeroU32),
    ServerInitiated(NonZeroU32),
}

impl StreamId {
    /// Creates an odd, nonzero client-owned stream identity.
    ///
    /// # Errors
    ///
    /// Returns [`FrameError::InvalidClientStream`] when `value` is even.
    pub fn client_initiated(value: NonZeroU32) -> Result<Self, FrameError> {
        if value.get().is_multiple_of(2) {
            Err(FrameError::InvalidClientStream(value.get()))
        } else {
            Ok(Self::ClientInitiated(value))
        }
    }

    /// Creates an even, nonzero server-owned stream identity.
    ///
    /// # Errors
    ///
    /// Returns [`FrameError::InvalidServerStream`] when `value` is odd.
    pub fn server_initiated(value: NonZeroU32) -> Result<Self, FrameError> {
        if value.get().is_multiple_of(2) {
            Ok(Self::ServerInitiated(value))
        } else {
            Err(FrameError::InvalidServerStream(value.get()))
        }
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        match self {
            Self::Control => 0,
            Self::ClientInitiated(value) | Self::ServerInitiated(value) => value.get(),
        }
    }

    pub(crate) fn decode(value: u32) -> Self {
        match NonZeroU32::new(value) {
            None => Self::Control,
            Some(value) if value.get().is_multiple_of(2) => Self::ServerInitiated(value),
            Some(value) => Self::ClientInitiated(value),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_one_preface_round_trips_and_rejects_other_versions() -> Result<(), FrameError> {
        let encoded = ProtocolPreface.encode();
        assert_eq!(ProtocolPreface::decode(&encoded)?, ProtocolPreface);

        let mut version_two = encoded;
        version_two[5] = 2;
        assert_eq!(
            ProtocolPreface::decode(&version_two),
            Err(FrameError::InvalidPreface)
        );
        Ok(())
    }
}
