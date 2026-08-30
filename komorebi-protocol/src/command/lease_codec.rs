use std::num::NonZeroU32;

use minicbor::Decoder;
use minicbor::Encoder;
use minicbor::data::Type;

use super::CommandCodecError;
use super::InvocationLeaseRejection;
use super::InvocationLeaseReply;
use super::InvocationLeaseRequest;
use super::codec::MAX_COMMAND_PAYLOAD_BYTES;
use super::codec::bounded_map;
use super::codec::decode_bytes;
use super::codec::required;
use super::codec::skip_bounded;
use super::codec::unique_key;
use crate::InvocationLease;
use crate::InvocationNamespaceId;
use crate::InvocationSequence;

const ISSUED_REPLY_TAG: u8 = 1;
const REJECTED_REPLY_TAG: u8 = 2;

enum ReplyFieldOne {
    Namespace(InvocationNamespaceId),
    Rejection(u8),
}

#[derive(Clone, Copy, Debug, Default)]
pub struct InvocationLeaseCodec;

impl InvocationLeaseCodec {
    /// Encodes a canonical request for a new or existing invocation namespace.
    ///
    /// # Errors
    ///
    /// Returns [`CommandCodecError`] when CBOR encoding fails.
    pub fn encode_request(request: InvocationLeaseRequest) -> Result<Vec<u8>, CommandCodecError> {
        let mut encoder = Encoder::new(Vec::with_capacity(32));
        encoder.map(if request.namespace().is_some() { 2 } else { 1 })?;
        encoder.u8(0)?.u32(request.count().get())?;
        if let Some(namespace) = request.namespace() {
            encoder.u8(1)?.bytes(&namespace.into_bytes())?;
        }
        bounded(encoder.into_writer())
    }

    /// Decodes a strict, bounded invocation lease request.
    ///
    /// # Errors
    ///
    /// Returns [`CommandCodecError`] for malformed, duplicate, missing,
    /// oversized, indefinite, or trailing input.
    pub fn decode_request(bytes: &[u8]) -> Result<InvocationLeaseRequest, CommandCodecError> {
        ensure_bound(bytes)?;
        let mut decoder = Decoder::new(bytes);
        let count = bounded_map(&mut decoder)?;
        let mut seen = [false; 256];
        let mut lease_count = None;
        let mut namespace = None;
        for _ in 0..count {
            match unique_key(&mut decoder, &mut seen)? {
                0 => {
                    lease_count = Some(
                        NonZeroU32::new(decoder.u32()?).ok_or(CommandCodecError::ZeroLeaseCount)?,
                    );
                }
                1 => {
                    namespace = Some(InvocationNamespaceId::new(decode_bytes(&mut decoder)?)?);
                }
                _ => skip_bounded(&mut decoder, 0)?,
            }
        }
        require_eof(&decoder, bytes)?;
        Ok(InvocationLeaseRequest::new(
            namespace,
            required(lease_count, 0)?,
        ))
    }

    /// Encodes an issued lease or a stable lease rejection.
    ///
    /// # Errors
    ///
    /// Returns [`CommandCodecError`] when CBOR encoding fails.
    pub fn encode_reply(reply: InvocationLeaseReply) -> Result<Vec<u8>, CommandCodecError> {
        let mut encoder = Encoder::new(Vec::with_capacity(64));
        match reply {
            InvocationLeaseReply::Issued(lease) => {
                encoder
                    .map(5)?
                    .u8(0)?
                    .u8(ISSUED_REPLY_TAG)?
                    .u8(1)?
                    .bytes(&lease.namespace().into_bytes())?
                    .u8(2)?
                    .u64(lease.first().get())?
                    .u8(3)?
                    .u32(lease.count().get())?
                    .u8(4)?
                    .u64(lease.minimum_accepted().get())?;
            }
            InvocationLeaseReply::Rejected(reason) => {
                encoder
                    .map(2)?
                    .u8(0)?
                    .u8(REJECTED_REPLY_TAG)?
                    .u8(1)?
                    .u8(reason as u8)?;
            }
        }
        bounded(encoder.into_writer())
    }

    /// Decodes a strict, bounded invocation lease reply.
    ///
    /// # Errors
    ///
    /// Returns [`CommandCodecError`] for malformed, duplicate, missing,
    /// unknown, oversized, indefinite, or trailing input.
    pub fn decode_reply(bytes: &[u8]) -> Result<InvocationLeaseReply, CommandCodecError> {
        ensure_bound(bytes)?;
        let mut decoder = Decoder::new(bytes);
        let count = bounded_map(&mut decoder)?;
        let mut seen = [false; 256];
        let mut tag = None;
        let mut field_one = None;
        let mut first = None;
        let mut lease_count = None;
        let mut minimum_accepted = None;
        for _ in 0..count {
            match unique_key(&mut decoder, &mut seen)? {
                0 => tag = Some(decoder.u8()?),
                1 => {
                    field_one = Some(match decoder.datatype()? {
                        Type::Bytes => ReplyFieldOne::Namespace(InvocationNamespaceId::new(
                            decode_bytes(&mut decoder)?,
                        )?),
                        Type::U8 | Type::U16 | Type::U32 | Type::U64 => {
                            ReplyFieldOne::Rejection(decoder.u8()?)
                        }
                        _ => return Err(CommandCodecError::WrongLeaseReplyFieldType),
                    });
                }
                2 => {
                    require_issued_tag(tag, 2)?;
                    first = Some(InvocationSequence::try_from(decoder.u64()?)?);
                }
                3 => {
                    require_issued_tag(tag, 3)?;
                    lease_count = Some(
                        NonZeroU32::new(decoder.u32()?).ok_or(CommandCodecError::ZeroLeaseCount)?,
                    );
                }
                4 => {
                    require_issued_tag(tag, 4)?;
                    minimum_accepted = Some(InvocationSequence::try_from(decoder.u64()?)?);
                }
                _ => skip_bounded(&mut decoder, 0)?,
            }
        }
        require_eof(&decoder, bytes)?;
        match required(tag, 0)? {
            ISSUED_REPLY_TAG => {
                let ReplyFieldOne::Namespace(namespace) = required(field_one, 1)? else {
                    return Err(CommandCodecError::WrongLeaseReplyFieldType);
                };
                Ok(InvocationLeaseReply::Issued(InvocationLease::new(
                    namespace,
                    required(first, 2)?,
                    required(lease_count, 3)?,
                    required(minimum_accepted, 4)?,
                )))
            }
            REJECTED_REPLY_TAG => {
                let ReplyFieldOne::Rejection(reason) = required(field_one, 1)? else {
                    return Err(CommandCodecError::WrongLeaseReplyFieldType);
                };
                InvocationLeaseRejection::decode(reason)
                    .map(InvocationLeaseReply::Rejected)
                    .ok_or(CommandCodecError::UnknownLeaseRejection(reason))
            }
            tag => Err(CommandCodecError::UnknownLeaseReplyTag(tag)),
        }
    }
}

fn require_issued_tag(tag: Option<u8>, key: u8) -> Result<(), CommandCodecError> {
    match tag.ok_or(CommandCodecError::MissingKey(0))? {
        ISSUED_REPLY_TAG => Ok(()),
        tag => Err(CommandCodecError::UnexpectedLeaseReplyField { tag, key }),
    }
}

fn bounded(bytes: Vec<u8>) -> Result<Vec<u8>, CommandCodecError> {
    ensure_bound(&bytes)?;
    Ok(bytes)
}

fn ensure_bound(bytes: &[u8]) -> Result<(), CommandCodecError> {
    if bytes.len() > MAX_COMMAND_PAYLOAD_BYTES {
        Err(CommandCodecError::PayloadTooLarge(bytes.len()))
    } else {
        Ok(())
    }
}

fn require_eof(decoder: &Decoder<'_>, bytes: &[u8]) -> Result<(), CommandCodecError> {
    if decoder.position() == bytes.len() {
        Ok(())
    } else {
        Err(CommandCodecError::TrailingBytes)
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn namespace() -> Result<InvocationNamespaceId, crate::InvocationIdentityError> {
        InvocationNamespaceId::new([1; 16])
    }

    fn issued() -> Result<InvocationLeaseReply, crate::InvocationIdentityError> {
        Ok(InvocationLeaseReply::Issued(InvocationLease::new(
            namespace()?,
            InvocationSequence::try_from(7)?,
            NonZeroU32::MIN.saturating_add(2),
            InvocationSequence::try_from(4)?,
        )))
    }

    #[test]
    fn lease_request_round_trips_canonically() -> Result<(), Box<dyn std::error::Error>> {
        let request =
            InvocationLeaseRequest::new(Some(namespace()?), NonZeroU32::MIN.saturating_add(2));
        let encoded = InvocationLeaseCodec::encode_request(request)?;
        let mut fixture = vec![0xA2, 0x00, 0x03, 0x01, 0x50];
        fixture.extend_from_slice(&[1; 16]);
        assert_eq!(encoded, fixture);
        assert_eq!(InvocationLeaseCodec::decode_request(&encoded)?, request);

        let fresh = InvocationLeaseRequest::new(None, NonZeroU32::MIN);
        assert_eq!(
            InvocationLeaseCodec::encode_request(fresh)?,
            [0xA1, 0x00, 0x01]
        );
        assert_eq!(
            InvocationLeaseCodec::decode_request(&[0xA1, 0x00, 0x01])?,
            fresh
        );
        Ok(())
    }

    #[test]
    fn lease_replies_round_trip_canonically() -> Result<(), Box<dyn std::error::Error>> {
        let issued = issued()?;
        let encoded = InvocationLeaseCodec::encode_reply(issued)?;
        let mut fixture = vec![0xA5, 0x00, 0x01, 0x01, 0x50];
        fixture.extend_from_slice(&[1; 16]);
        fixture.extend_from_slice(&[0x02, 0x07, 0x03, 0x03, 0x04, 0x04]);
        assert_eq!(encoded, fixture);
        assert_eq!(InvocationLeaseCodec::decode_reply(&encoded)?, issued);

        let rejected = InvocationLeaseReply::Rejected(InvocationLeaseRejection::CapacityFull);
        assert_eq!(
            InvocationLeaseCodec::encode_reply(rejected)?,
            [0xA2, 0x00, 0x02, 0x01, 0x02]
        );
        assert_eq!(
            InvocationLeaseCodec::decode_reply(&[0xA2, 0x00, 0x02, 0x01, 0x02])?,
            rejected
        );
        Ok(())
    }

    #[test]
    fn lease_codec_rejects_ambiguous_or_noncanonical_known_fields() {
        assert!(matches!(
            InvocationLeaseCodec::decode_request(&[0xA2, 0x00, 0x01, 0x00, 0x02]),
            Err(CommandCodecError::DuplicateKey(0))
        ));
        assert!(matches!(
            InvocationLeaseCodec::decode_request(&[0xA1, 0x00, 0x00]),
            Err(CommandCodecError::ZeroLeaseCount)
        ));
        assert!(matches!(
            InvocationLeaseCodec::decode_reply(&[0xA3, 0x00, 0x02, 0x01, 0x02, 0x02, 0x01]),
            Err(CommandCodecError::UnexpectedLeaseReplyField { tag: 2, key: 2 })
        ));
        assert!(matches!(
            InvocationLeaseCodec::decode_reply(&[0xA2, 0x00, 0x02, 0x01, 0x18, 0x63]),
            Err(CommandCodecError::UnknownLeaseRejection(99))
        ));
    }

    #[test]
    fn lease_codec_skips_bounded_additive_fields() -> Result<(), CommandCodecError> {
        let bytes = [0xA2, 0x00, 0x01, 0x09, 0x82, 0xF5, 0x01];
        assert_eq!(
            InvocationLeaseCodec::decode_request(&bytes)?,
            InvocationLeaseRequest::new(None, NonZeroU32::MIN)
        );
        Ok(())
    }

    proptest! {
        #[test]
        fn arbitrary_lease_payloads_never_panic(bytes in prop::collection::vec(any::<u8>(), 0..20_000)) {
            let _ = InvocationLeaseCodec::decode_request(&bytes);
            let _ = InvocationLeaseCodec::decode_reply(&bytes);
        }
    }
}
