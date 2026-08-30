use crate::DirectionSequence;
use crate::FrameError;
use crate::FrameKind;
use crate::StreamId;

macro_rules! header_bytes {
    ($bytes:literal) => {
        pub const HEADER_BYTES: usize = $bytes;
        pub(crate) const HEADER_WIRE_BYTES: u32 = $bytes;
    };
}

header_bytes!(24);
pub const MAX_FRAME_PAYLOAD_BYTES: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PayloadLength(u32);

impl PayloadLength {
    fn from_usize(value: usize) -> Result<Self, FrameError> {
        if value > MAX_FRAME_PAYLOAD_BYTES {
            return Err(FrameError::PayloadTooLarge(value));
        }
        let value = u32::try_from(value).map_err(|_| FrameError::PayloadTooLarge(value))?;
        Ok(Self(value))
    }

    fn decode(value: u32) -> Result<Self, FrameError> {
        let value = usize::try_from(value)
            .map_err(|_| FrameError::PayloadTooLarge(MAX_FRAME_PAYLOAD_BYTES + 1))?;
        Self::from_usize(value)
    }

    const fn get(self) -> u32 {
        self.0
    }

    fn as_usize(self) -> usize {
        self.0 as usize
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameHeader {
    payload_len: PayloadLength,
    kind: FrameKind,
    stream_id: StreamId,
    sequence: DirectionSequence,
}

impl FrameHeader {
    fn for_payload(
        payload_len: usize,
        kind: FrameKind,
        stream_id: StreamId,
        sequence: DirectionSequence,
    ) -> Result<Self, FrameError> {
        Ok(Self {
            payload_len: PayloadLength::from_usize(payload_len)?,
            kind,
            stream_id,
            sequence,
        })
    }

    /// Decodes and validates one complete version 1 frame header.
    ///
    /// # Errors
    ///
    /// Returns a [`FrameError`] when the header length, payload bound, flags,
    /// or reserved field violates the version 1 framing contract.
    pub fn decode(bytes: &[u8]) -> Result<Self, FrameError> {
        let bytes: &[u8; HEADER_BYTES] = bytes
            .try_into()
            .map_err(|_| FrameError::HeaderLength(bytes.len()))?;
        let payload_len = PayloadLength::decode(u32::from_be_bytes(field(bytes, 0)?))?;
        let kind = FrameKind::new(u16::from_be_bytes(field(bytes, 4)?));
        let flags = u16::from_be_bytes(field(bytes, 6)?);
        if flags != 0 {
            return Err(FrameError::UnknownFlags(flags));
        }
        let stream_id = StreamId::decode(u32::from_be_bytes(field(bytes, 8)?));
        let sequence = DirectionSequence::try_from(u64::from_be_bytes(field(bytes, 12)?))?;
        let reserved = u32::from_be_bytes(field(bytes, 20)?);
        if reserved != 0 {
            return Err(FrameError::ReservedField(reserved));
        }
        Ok(Self {
            payload_len,
            kind,
            stream_id,
            sequence,
        })
    }

    #[must_use]
    pub fn encode(self) -> [u8; HEADER_BYTES] {
        let mut bytes = [0; HEADER_BYTES];
        bytes[0..4].copy_from_slice(&self.payload_len.get().to_be_bytes());
        bytes[4..6].copy_from_slice(&self.kind.get().to_be_bytes());
        bytes[8..12].copy_from_slice(&self.stream_id.get().to_be_bytes());
        bytes[12..20].copy_from_slice(&self.sequence.get().to_be_bytes());
        bytes
    }

    #[must_use]
    pub fn payload_len(self) -> usize {
        self.payload_len.as_usize()
    }

    #[must_use]
    pub const fn kind(self) -> FrameKind {
        self.kind
    }

    #[must_use]
    pub const fn stream_id(self) -> StreamId {
        self.stream_id
    }

    #[must_use]
    pub const fn sequence(self) -> DirectionSequence {
        self.sequence
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Frame {
    header: FrameHeader,
    payload: Box<[u8]>,
}

impl Frame {
    /// Owns a payload and derives its bounded header length.
    ///
    /// # Errors
    ///
    /// Returns [`FrameError::PayloadTooLarge`] when the payload exceeds the
    /// absolute version 1 frame limit.
    pub fn new(
        kind: FrameKind,
        stream_id: StreamId,
        sequence: DirectionSequence,
        payload: impl Into<Box<[u8]>>,
    ) -> Result<Self, FrameError> {
        let payload = payload.into();
        let header = FrameHeader::for_payload(payload.len(), kind, stream_id, sequence)?;
        Ok(Self { header, payload })
    }

    /// Joins a decoded header to an already-read payload.
    ///
    /// # Errors
    ///
    /// Returns [`FrameError::PayloadLengthMismatch`] when the received payload
    /// length differs from the validated header declaration.
    pub fn from_received_parts(
        header: FrameHeader,
        payload: impl Into<Box<[u8]>>,
    ) -> Result<Self, FrameError> {
        let payload = payload.into();
        if header.payload_len() != payload.len() {
            return Err(FrameError::PayloadLengthMismatch {
                declared: header.payload_len(),
                actual: payload.len(),
            });
        }
        Ok(Self { header, payload })
    }

    #[must_use]
    pub const fn header(&self) -> FrameHeader {
        self.header
    }

    #[must_use]
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }

    #[must_use]
    pub fn into_payload(self) -> Box<[u8]> {
        self.payload
    }
}

fn field<const N: usize>(bytes: &[u8], offset: usize) -> Result<[u8; N], FrameError> {
    let end = offset.checked_add(N).ok_or(FrameError::HeaderField)?;
    bytes
        .get(offset..end)
        .ok_or(FrameError::HeaderField)?
        .try_into()
        .map_err(|_| FrameError::HeaderField)
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use proptest::prelude::*;

    use super::*;

    proptest! {
        #[test]
        fn arbitrary_header_bytes_never_panic(bytes in prop::collection::vec(any::<u8>(), 0..80)) {
            let _ = FrameHeader::decode(&bytes);
        }

        #[test]
        fn valid_headers_round_trip(
            payload_len in 0_usize..=MAX_FRAME_PAYLOAD_BYTES,
            kind in any::<u16>(),
            stream in any::<u32>(),
            sequence in 1_u64..=u64::MAX,
        ) {
            let header = FrameHeader::for_payload(
                payload_len,
                FrameKind::new(kind),
                StreamId::decode(stream),
                DirectionSequence::try_from(sequence)?,
            )?;
            prop_assert_eq!(FrameHeader::decode(&header.encode())?, header);
        }
    }

    #[test]
    fn frame_derives_length_from_owned_payload() -> Result<(), FrameError> {
        let value = NonZeroU32::new(3).ok_or(FrameError::HeaderField)?;
        let frame = Frame::new(
            FrameKind::new(7),
            StreamId::client_initiated(value)?,
            DirectionSequence::try_from(11)?,
            vec![1, 2, 3],
        )?;

        assert_eq!(frame.header().payload_len(), 3);
        assert_eq!(frame.payload(), [1, 2, 3]);
        Ok(())
    }

    #[test]
    fn received_parts_reject_a_length_mismatch() -> Result<(), FrameError> {
        let header = FrameHeader::for_payload(
            4,
            FrameKind::new(1),
            StreamId::Control,
            DirectionSequence::try_from(1)?,
        )?;
        assert_eq!(
            Frame::from_received_parts(header, vec![1, 2, 3]),
            Err(FrameError::PayloadLengthMismatch {
                declared: 4,
                actual: 3,
            })
        );
        Ok(())
    }

    #[test]
    fn version_one_rejects_flags_and_reserved_bytes() {
        let mut flags = [0; HEADER_BYTES];
        flags[7] = 1;
        assert_eq!(
            FrameHeader::decode(&flags),
            Err(FrameError::UnknownFlags(1))
        );

        let mut reserved = [0; HEADER_BYTES];
        reserved[19] = 1;
        reserved[23] = 1;
        assert_eq!(
            FrameHeader::decode(&reserved),
            Err(FrameError::ReservedField(1))
        );
    }

    #[test]
    fn version_one_rejects_zero_direction_sequence() {
        assert_eq!(
            FrameHeader::decode(&[0; HEADER_BYTES]),
            Err(FrameError::ZeroDirectionSequence)
        );
    }

    #[test]
    fn header_rejects_payload_lengths_above_the_absolute_bound()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut bytes = [0; HEADER_BYTES];
        let invalid = u32::try_from(MAX_FRAME_PAYLOAD_BYTES + 1)?;
        bytes[0..4].copy_from_slice(&invalid.to_be_bytes());

        assert_eq!(
            FrameHeader::decode(&bytes),
            Err(FrameError::PayloadTooLarge(MAX_FRAME_PAYLOAD_BYTES + 1))
        );
        Ok(())
    }

    #[test]
    fn stream_parity_is_checked_at_construction() -> Result<(), FrameError> {
        let odd = NonZeroU32::new(3).ok_or(FrameError::HeaderField)?;
        let even = NonZeroU32::new(4).ok_or(FrameError::HeaderField)?;

        assert_eq!(StreamId::client_initiated(odd)?.get(), 3);
        assert_eq!(StreamId::server_initiated(even)?.get(), 4);
        assert_eq!(
            StreamId::client_initiated(even),
            Err(FrameError::InvalidClientStream(4))
        );
        assert_eq!(
            StreamId::server_initiated(odd),
            Err(FrameError::InvalidServerStream(3))
        );
        Ok(())
    }
}
