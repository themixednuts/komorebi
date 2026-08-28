use minicbor::{Decoder, Encoder, data::Type};
use thiserror::Error;

pub const PREFACE: [u8; 8] = [b'K', b'C', b'M', b'D', 0, 1, 0, 0];
pub const HEADER_BYTES: usize = 24;
pub const MAX_PAYLOAD_BYTES: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameHeader {
    pub payload_len: u32,
    pub kind: u16,
    pub flags: u16,
    pub stream_id: u32,
    pub sequence: u64,
}

impl FrameHeader {
    pub fn encode(self) -> [u8; HEADER_BYTES] {
        let mut bytes = [0_u8; HEADER_BYTES];
        bytes[0..4].copy_from_slice(&self.payload_len.to_be_bytes());
        bytes[4..6].copy_from_slice(&self.kind.to_be_bytes());
        bytes[6..8].copy_from_slice(&self.flags.to_be_bytes());
        bytes[8..12].copy_from_slice(&self.stream_id.to_be_bytes());
        bytes[12..20].copy_from_slice(&self.sequence.to_be_bytes());
        bytes
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, FrameError> {
        let bytes: &[u8; HEADER_BYTES] = bytes.try_into().map_err(|_| FrameError::HeaderLength)?;
        let payload_len = u32::from_be_bytes(array(bytes, 0)?);
        let kind = u16::from_be_bytes(array(bytes, 4)?);
        let flags = u16::from_be_bytes(array(bytes, 6)?);
        let stream_id = u32::from_be_bytes(array(bytes, 8)?);
        let sequence = u64::from_be_bytes(array(bytes, 12)?);
        let reserved = u32::from_be_bytes(array(bytes, 20)?);
        if usize::try_from(payload_len).map_err(|_| FrameError::PayloadTooLarge)?
            > MAX_PAYLOAD_BYTES
        {
            return Err(FrameError::PayloadTooLarge);
        }
        if flags != 0 {
            return Err(FrameError::UnknownFlags(flags));
        }
        if reserved != 0 {
            return Err(FrameError::Reserved);
        }
        Ok(Self {
            payload_len,
            kind,
            flags,
            stream_id,
            sequence,
        })
    }
}

fn array<const N: usize>(bytes: &[u8], offset: usize) -> Result<[u8; N], FrameError> {
    bytes
        .get(offset..offset.checked_add(N).ok_or(FrameError::HeaderLength)?)
        .ok_or(FrameError::HeaderLength)?
        .try_into()
        .map_err(|_| FrameError::HeaderLength)
}

pub fn encode_noop(sequence: u64) -> Result<Vec<u8>, FrameError> {
    let mut encoder = Encoder::new(Vec::with_capacity(12));
    encoder.map(1)?.u8(0)?.u64(sequence)?;
    Ok(encoder.into_writer())
}

pub fn decode_noop(bytes: &[u8]) -> Result<u64, FrameError> {
    let mut decoder = Decoder::new(bytes);
    let length = decoder.map()?.ok_or(FrameError::Indefinite)?;
    if length != 1 {
        return Err(FrameError::MapShape);
    }
    if decoder.datatype()? != Type::U8 || decoder.u8()? != 0 {
        return Err(FrameError::UnknownKey);
    }
    let value = decoder.u64()?;
    if decoder.position() != bytes.len() {
        return Err(FrameError::TrailingBytes);
    }
    Ok(value)
}

pub fn encode_action_offers(count: usize) -> Result<Vec<u8>, FrameError> {
    let count = u64::try_from(count).map_err(|_| FrameError::CountRange)?;
    let mut encoder = Encoder::new(Vec::with_capacity(32_768));
    encoder.array(count)?;
    for index in 0..count {
        encoder
            .map(3)?
            .u8(0)?
            .u64(index)?
            .u8(1)?
            .str("window.focus")?
            .u8(2)?
            .bool(index % 2 == 0)?;
    }
    Ok(encoder.into_writer())
}

#[derive(Debug, Error)]
pub enum FrameError {
    #[error("frame header is not exactly 24 bytes")]
    HeaderLength,
    #[error("frame payload exceeds one MiB")]
    PayloadTooLarge,
    #[error("frame contains unknown flags {0:#x}")]
    UnknownFlags(u16),
    #[error("frame reserved field is nonzero")]
    Reserved,
    #[error("CBOR map must use definite length")]
    Indefinite,
    #[error("CBOR map has the wrong shape")]
    MapShape,
    #[error("CBOR map contains an unknown or nonnumeric key")]
    UnknownKey,
    #[error("CBOR payload has trailing bytes")]
    TrailingBytes,
    #[error("collection length is outside CBOR range")]
    CountRange,
    #[error(transparent)]
    Encode(#[from] minicbor::encode::Error<std::convert::Infallible>),
    #[error(transparent)]
    Decode(#[from] minicbor::decode::Error),
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::{FrameHeader, HEADER_BYTES, decode_noop, encode_noop};

    proptest! {
        #[test]
        fn arbitrary_headers_never_panic(bytes in prop::collection::vec(any::<u8>(), 0..80)) {
            let _ = FrameHeader::decode(&bytes);
        }

        #[test]
        fn arbitrary_payloads_never_panic_or_allocate_from_claimed_lengths(
            bytes in prop::collection::vec(any::<u8>(), 0..4096)
        ) {
            let _ = decode_noop(&bytes);
        }

        #[test]
        fn noop_round_trip(sequence in any::<u64>()) {
            let encoded = encode_noop(sequence)?;
            prop_assert_eq!(decode_noop(&encoded)?, sequence);
        }
    }

    #[test]
    fn header_rejects_reserved_bytes() {
        let mut bytes = [0_u8; HEADER_BYTES];
        bytes[23] = 1;
        assert!(FrameHeader::decode(&bytes).is_err());
    }
}
