use minicbor::Decoder;
use minicbor::Encoder;
use minicbor::data::Type;

use super::CancelInvocationReply;
use super::CancelInvocationRequest;
use super::CommandCodecError;
use super::InvocationProgress;
use super::InvocationStatus;
use super::InvocationStatusReply;
use super::InvocationStatusRequest;
use super::InvocationTerminal;
use super::InvocationUnavailable;
use super::SettledInvocationKind;
use super::codec::bounded_encoded;
use super::codec::bounded_map;
use super::codec::decode_invocation_id;
use super::codec::decode_state;
use super::codec::encode_invocation_id;
use super::codec::encode_state;
use super::codec::ensure_command_bound;
use super::codec::require_eof;
use super::codec::required;
use super::codec::skip_bounded;
use super::codec::unique_key;
use crate::InvocationDigest;

const RETAINED_STATUS_TAG: u8 = 1;
const UNAVAILABLE_STATUS_TAG: u8 = 2;
const CANCELLED_REPLY_TAG: u8 = 1;
const TOO_LATE_REPLY_TAG: u8 = 2;
const ALREADY_TERMINAL_REPLY_TAG: u8 = 3;
const UNAVAILABLE_CANCEL_REPLY_TAG: u8 = 4;
const RESERVED_PROGRESS_TAG: u8 = 1;
const LOGICAL_COMMITTED_PROGRESS_TAG: u8 = 2;
const EFFECT_DISPATCHED_PROGRESS_TAG: u8 = 3;
const TERMINAL_PROGRESS_TAG: u8 = 4;
const SETTLED_TERMINAL_TAG: u8 = 1;
const CANCELLED_BEFORE_COMMIT_TERMINAL_TAG: u8 = 2;
const RESTARTED_BEFORE_COMMIT_TERMINAL_TAG: u8 = 3;

enum ReplyBody {
    Status(InvocationStatus),
    Unavailable(u8),
}

#[derive(Clone, Copy, Debug, Default)]
pub struct InvocationControlCodec;

#[derive(Clone, Copy, Debug, Default)]
pub struct InvocationStatusCodec;

impl InvocationStatusCodec {
    /// Encodes the canonical status value used by durable committed-event and
    /// outcome documents.
    ///
    /// # Errors
    ///
    /// Returns [`CommandCodecError`] when CBOR encoding fails or exceeds the
    /// command payload bound.
    pub fn encode(status: InvocationStatus) -> Result<Vec<u8>, CommandCodecError> {
        let mut encoder = Encoder::new(Vec::with_capacity(88));
        encode_status(&mut encoder, status)?;
        bounded_encoded(encoder.into_writer())
    }

    /// Decodes one strict canonical status value.
    ///
    /// # Errors
    ///
    /// Returns [`CommandCodecError`] for malformed, duplicate, missing,
    /// oversized, indefinite, unknown, or trailing input.
    pub fn decode(bytes: &[u8]) -> Result<InvocationStatus, CommandCodecError> {
        ensure_command_bound(bytes)?;
        let mut decoder = Decoder::new(bytes);
        let status = decode_status(&mut decoder)?;
        require_eof(&decoder, bytes)?;
        Ok(status)
    }
}

impl InvocationControlCodec {
    /// Encodes a canonical invocation status request.
    ///
    /// # Errors
    ///
    /// Returns [`CommandCodecError`] when CBOR encoding fails.
    pub fn encode_status_request(
        request: InvocationStatusRequest,
    ) -> Result<Vec<u8>, CommandCodecError> {
        encode_request(request.invocation_id())
    }

    /// Decodes a strict, bounded invocation status request.
    ///
    /// # Errors
    ///
    /// Returns [`CommandCodecError`] for malformed, duplicate, missing,
    /// oversized, indefinite, or trailing input.
    pub fn decode_status_request(
        bytes: &[u8],
    ) -> Result<InvocationStatusRequest, CommandCodecError> {
        decode_request(bytes).map(InvocationStatusRequest::new)
    }

    /// Encodes a retained or unavailable invocation status.
    ///
    /// # Errors
    ///
    /// Returns [`CommandCodecError`] when CBOR encoding fails.
    pub fn encode_status_reply(reply: InvocationStatusReply) -> Result<Vec<u8>, CommandCodecError> {
        let mut encoder = Encoder::new(Vec::with_capacity(96));
        encoder.map(2)?.u8(0)?;
        match reply {
            InvocationStatusReply::Retained(status) => {
                encoder.u8(RETAINED_STATUS_TAG)?.u8(1)?;
                encode_status(&mut encoder, status)?;
            }
            InvocationStatusReply::Unavailable(reason) => {
                encoder
                    .u8(UNAVAILABLE_STATUS_TAG)?
                    .u8(1)?
                    .u8(reason as u8)?;
            }
        }
        bounded_encoded(encoder.into_writer())
    }

    /// Decodes a strict, bounded invocation status reply.
    ///
    /// # Errors
    ///
    /// Returns [`CommandCodecError`] for malformed, duplicate, missing,
    /// unknown, oversized, indefinite, or trailing input.
    pub fn decode_status_reply(bytes: &[u8]) -> Result<InvocationStatusReply, CommandCodecError> {
        let (tag, body) = decode_reply(bytes)?;
        match (tag, body) {
            (RETAINED_STATUS_TAG, ReplyBody::Status(status)) => {
                Ok(InvocationStatusReply::Retained(status))
            }
            (UNAVAILABLE_STATUS_TAG, ReplyBody::Unavailable(reason)) => {
                InvocationUnavailable::decode(reason)
                    .map(InvocationStatusReply::Unavailable)
                    .ok_or(CommandCodecError::UnknownInvocationUnavailable(reason))
            }
            (RETAINED_STATUS_TAG | UNAVAILABLE_STATUS_TAG, _) => {
                Err(CommandCodecError::WrongInvocationControlReplyFieldType(tag))
            }
            (tag, _) => Err(CommandCodecError::UnknownStatusReplyTag(tag)),
        }
    }

    /// Encodes a canonical advisory cancellation request.
    ///
    /// # Errors
    ///
    /// Returns [`CommandCodecError`] when CBOR encoding fails.
    pub fn encode_cancel_request(
        request: CancelInvocationRequest,
    ) -> Result<Vec<u8>, CommandCodecError> {
        encode_request(request.invocation_id())
    }

    /// Decodes a strict, bounded advisory cancellation request.
    ///
    /// # Errors
    ///
    /// Returns [`CommandCodecError`] for malformed, duplicate, missing,
    /// oversized, indefinite, or trailing input.
    pub fn decode_cancel_request(
        bytes: &[u8],
    ) -> Result<CancelInvocationRequest, CommandCodecError> {
        decode_request(bytes).map(CancelInvocationRequest::new)
    }

    /// Encodes the single durable winner of an advisory cancellation race.
    ///
    /// # Errors
    ///
    /// Returns [`CommandCodecError`] when CBOR encoding fails.
    pub fn encode_cancel_reply(reply: CancelInvocationReply) -> Result<Vec<u8>, CommandCodecError> {
        let mut encoder = Encoder::new(Vec::with_capacity(96));
        encoder.map(2)?.u8(0)?;
        match reply {
            CancelInvocationReply::Cancelled(status) => {
                encoder.u8(CANCELLED_REPLY_TAG)?.u8(1)?;
                encode_status(&mut encoder, status)?;
            }
            CancelInvocationReply::TooLate(status) => {
                encoder.u8(TOO_LATE_REPLY_TAG)?.u8(1)?;
                encode_status(&mut encoder, status)?;
            }
            CancelInvocationReply::AlreadyTerminal(status) => {
                encoder.u8(ALREADY_TERMINAL_REPLY_TAG)?.u8(1)?;
                encode_status(&mut encoder, status)?;
            }
            CancelInvocationReply::Unavailable(reason) => {
                encoder
                    .u8(UNAVAILABLE_CANCEL_REPLY_TAG)?
                    .u8(1)?
                    .u8(reason as u8)?;
            }
        }
        bounded_encoded(encoder.into_writer())
    }

    /// Decodes a strict, bounded advisory cancellation reply.
    ///
    /// # Errors
    ///
    /// Returns [`CommandCodecError`] for malformed, duplicate, missing,
    /// unknown, oversized, indefinite, or trailing input.
    pub fn decode_cancel_reply(bytes: &[u8]) -> Result<CancelInvocationReply, CommandCodecError> {
        let (tag, body) = decode_reply(bytes)?;
        match (tag, body) {
            (CANCELLED_REPLY_TAG, ReplyBody::Status(status)) => {
                Ok(CancelInvocationReply::Cancelled(status))
            }
            (TOO_LATE_REPLY_TAG, ReplyBody::Status(status)) => {
                Ok(CancelInvocationReply::TooLate(status))
            }
            (ALREADY_TERMINAL_REPLY_TAG, ReplyBody::Status(status)) => {
                Ok(CancelInvocationReply::AlreadyTerminal(status))
            }
            (UNAVAILABLE_CANCEL_REPLY_TAG, ReplyBody::Unavailable(reason)) => {
                InvocationUnavailable::decode(reason)
                    .map(CancelInvocationReply::Unavailable)
                    .ok_or(CommandCodecError::UnknownInvocationUnavailable(reason))
            }
            (
                CANCELLED_REPLY_TAG
                | TOO_LATE_REPLY_TAG
                | ALREADY_TERMINAL_REPLY_TAG
                | UNAVAILABLE_CANCEL_REPLY_TAG,
                _,
            ) => Err(CommandCodecError::WrongInvocationControlReplyFieldType(tag)),
            (tag, _) => Err(CommandCodecError::UnknownCancelReplyTag(tag)),
        }
    }
}

fn encode_request(invocation_id: crate::InvocationId) -> Result<Vec<u8>, CommandCodecError> {
    let mut encoder = Encoder::new(Vec::with_capacity(32));
    encoder.map(1)?.u8(0)?;
    encode_invocation_id(&mut encoder, invocation_id)?;
    bounded_encoded(encoder.into_writer())
}

fn decode_request(bytes: &[u8]) -> Result<crate::InvocationId, CommandCodecError> {
    ensure_command_bound(bytes)?;
    let mut decoder = Decoder::new(bytes);
    let count = bounded_map(&mut decoder)?;
    let mut seen = [false; 256];
    let mut invocation_id = None;
    for _ in 0..count {
        match unique_key(&mut decoder, &mut seen)? {
            0 => invocation_id = Some(decode_invocation_id(&mut decoder)?),
            _ => skip_bounded(&mut decoder, 0)?,
        }
    }
    require_eof(&decoder, bytes)?;
    required(invocation_id, 0)
}

pub(super) fn encode_status(
    encoder: &mut Encoder<Vec<u8>>,
    status: InvocationStatus,
) -> Result<(), CommandCodecError> {
    encoder.map(3)?.u8(0)?;
    encode_invocation_id(encoder, status.invocation_id())?;
    encoder.u8(1)?.bytes(&status.digest().into_bytes())?.u8(2)?;
    encode_progress(encoder, status.progress())
}

pub(super) fn decode_status(
    decoder: &mut Decoder<'_>,
) -> Result<InvocationStatus, CommandCodecError> {
    let count = bounded_map(decoder)?;
    let mut seen = [false; 256];
    let mut invocation_id = None;
    let mut digest = None;
    let mut progress = None;
    for _ in 0..count {
        match unique_key(decoder, &mut seen)? {
            0 => invocation_id = Some(decode_invocation_id(decoder)?),
            1 => digest = Some(InvocationDigest::new(super::codec::decode_bytes(decoder)?)?),
            2 => progress = Some(decode_progress(decoder)?),
            _ => skip_bounded(decoder, 0)?,
        }
    }
    Ok(InvocationStatus::new(
        required(invocation_id, 0)?,
        required(digest, 1)?,
        required(progress, 2)?,
    ))
}

fn encode_progress(
    encoder: &mut Encoder<Vec<u8>>,
    progress: InvocationProgress,
) -> Result<(), CommandCodecError> {
    match progress {
        InvocationProgress::Reserved => {
            encoder.array(1)?.u8(RESERVED_PROGRESS_TAG)?;
        }
        InvocationProgress::LogicalCommitted(state) => {
            encoder.array(2)?.u8(LOGICAL_COMMITTED_PROGRESS_TAG)?;
            encode_state(encoder, state)?;
        }
        InvocationProgress::EffectDispatched(state) => {
            encoder.array(2)?.u8(EFFECT_DISPATCHED_PROGRESS_TAG)?;
            encode_state(encoder, state)?;
        }
        InvocationProgress::Terminal(terminal) => {
            encoder.array(2)?.u8(TERMINAL_PROGRESS_TAG)?;
            encode_terminal(encoder, terminal)?;
        }
    }
    Ok(())
}

fn decode_progress(decoder: &mut Decoder<'_>) -> Result<InvocationProgress, CommandCodecError> {
    let length = super::codec::definite(decoder.array()?)?;
    let tag = decoder.u8()?;
    match tag {
        RESERVED_PROGRESS_TAG if length == 1 => Ok(InvocationProgress::Reserved),
        LOGICAL_COMMITTED_PROGRESS_TAG if length == 2 => {
            Ok(InvocationProgress::LogicalCommitted(decode_state(decoder)?))
        }
        EFFECT_DISPATCHED_PROGRESS_TAG if length == 2 => {
            Ok(InvocationProgress::EffectDispatched(decode_state(decoder)?))
        }
        TERMINAL_PROGRESS_TAG if length == 2 => {
            Ok(InvocationProgress::Terminal(decode_terminal(decoder)?))
        }
        RESERVED_PROGRESS_TAG
        | LOGICAL_COMMITTED_PROGRESS_TAG
        | EFFECT_DISPATCHED_PROGRESS_TAG
        | TERMINAL_PROGRESS_TAG => {
            Err(CommandCodecError::WrongInvocationProgressLength { tag, length })
        }
        _ => Err(CommandCodecError::UnknownInvocationProgressTag(tag)),
    }
}

fn encode_terminal(
    encoder: &mut Encoder<Vec<u8>>,
    terminal: InvocationTerminal,
) -> Result<(), CommandCodecError> {
    match terminal {
        InvocationTerminal::Settled { state, kind } => {
            encoder.array(3)?.u8(SETTLED_TERMINAL_TAG)?.u8(kind as u8)?;
            encode_state(encoder, state)?;
        }
        InvocationTerminal::CancelledBeforeCommit => {
            encoder.array(1)?.u8(CANCELLED_BEFORE_COMMIT_TERMINAL_TAG)?;
        }
        InvocationTerminal::RestartedBeforeCommit => {
            encoder.array(1)?.u8(RESTARTED_BEFORE_COMMIT_TERMINAL_TAG)?;
        }
    }
    Ok(())
}

fn decode_terminal(decoder: &mut Decoder<'_>) -> Result<InvocationTerminal, CommandCodecError> {
    let length = super::codec::definite(decoder.array()?)?;
    let tag = decoder.u8()?;
    match tag {
        SETTLED_TERMINAL_TAG if length == 3 => {
            let kind = decoder.u8()?;
            Ok(InvocationTerminal::Settled {
                state: decode_state(decoder)?,
                kind: SettledInvocationKind::decode(kind)
                    .ok_or(CommandCodecError::UnknownSettledInvocationKind(kind))?,
            })
        }
        CANCELLED_BEFORE_COMMIT_TERMINAL_TAG if length == 1 => {
            Ok(InvocationTerminal::CancelledBeforeCommit)
        }
        RESTARTED_BEFORE_COMMIT_TERMINAL_TAG if length == 1 => {
            Ok(InvocationTerminal::RestartedBeforeCommit)
        }
        SETTLED_TERMINAL_TAG
        | CANCELLED_BEFORE_COMMIT_TERMINAL_TAG
        | RESTARTED_BEFORE_COMMIT_TERMINAL_TAG => {
            Err(CommandCodecError::WrongInvocationTerminalLength { tag, length })
        }
        _ => Err(CommandCodecError::UnknownInvocationTerminalTag(tag)),
    }
}

fn decode_reply(bytes: &[u8]) -> Result<(u8, ReplyBody), CommandCodecError> {
    ensure_command_bound(bytes)?;
    let mut decoder = Decoder::new(bytes);
    let count = bounded_map(&mut decoder)?;
    let mut seen = [false; 256];
    let mut tag = None;
    let mut body = None;
    for _ in 0..count {
        match unique_key(&mut decoder, &mut seen)? {
            0 => tag = Some(decoder.u8()?),
            1 => {
                let reply_tag = required(tag, 0)?;
                body = Some(match decoder.datatype()? {
                    Type::Map => ReplyBody::Status(decode_status(&mut decoder)?),
                    Type::U8 | Type::U16 | Type::U32 | Type::U64 => {
                        ReplyBody::Unavailable(decoder.u8()?)
                    }
                    _ => {
                        return Err(CommandCodecError::WrongInvocationControlReplyFieldType(
                            reply_tag,
                        ));
                    }
                });
            }
            _ => skip_bounded(&mut decoder, 0)?,
        }
    }
    require_eof(&decoder, bytes)?;
    Ok((required(tag, 0)?, required(body, 1)?))
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::InvocationId;
    use crate::InvocationNamespaceId;
    use crate::InvocationSequence;
    use crate::ManagerEpoch;
    use crate::Revision;
    use crate::StateStamp;

    fn invocation_id() -> Result<InvocationId, crate::InvocationIdentityError> {
        Ok(InvocationId::new(
            InvocationNamespaceId::new([1; 16])?,
            InvocationSequence::try_from(7)?,
        ))
    }

    fn state() -> Result<StateStamp, Box<dyn std::error::Error>> {
        Ok(StateStamp::new(
            ManagerEpoch::new([2; 16])?,
            Revision::try_from(11)?,
        ))
    }

    fn status(
        progress: InvocationProgress,
    ) -> Result<InvocationStatus, crate::InvocationIdentityError> {
        Ok(InvocationStatus::new(
            invocation_id()?,
            InvocationDigest::new([3; 32])?,
            progress,
        ))
    }

    #[test]
    fn status_and_cancel_requests_share_only_the_canonical_id_primitive()
    -> Result<(), Box<dyn std::error::Error>> {
        let id = invocation_id()?;
        let encoded =
            InvocationControlCodec::encode_status_request(InvocationStatusRequest::new(id))?;
        let mut fixture = vec![0xA1, 0x00, 0x82, 0x50];
        fixture.extend_from_slice(&[1; 16]);
        fixture.push(0x07);
        assert_eq!(encoded, fixture);
        assert_eq!(
            InvocationControlCodec::decode_status_request(&encoded)?,
            InvocationStatusRequest::new(id)
        );
        assert_eq!(
            InvocationControlCodec::decode_cancel_request(&encoded)?,
            CancelInvocationRequest::new(id)
        );
        Ok(())
    }

    #[test]
    fn every_type_safe_progress_state_round_trips() -> Result<(), Box<dyn std::error::Error>> {
        let progress = [
            InvocationProgress::Reserved,
            InvocationProgress::LogicalCommitted(state()?),
            InvocationProgress::EffectDispatched(state()?),
            InvocationProgress::Terminal(InvocationTerminal::Settled {
                state: state()?,
                kind: SettledInvocationKind::Succeeded,
            }),
            InvocationProgress::Terminal(InvocationTerminal::Settled {
                state: state()?,
                kind: SettledInvocationKind::Failed,
            }),
            InvocationProgress::Terminal(InvocationTerminal::Settled {
                state: state()?,
                kind: SettledInvocationKind::Degraded,
            }),
            InvocationProgress::Terminal(InvocationTerminal::Settled {
                state: state()?,
                kind: SettledInvocationKind::Indeterminate,
            }),
            InvocationProgress::Terminal(InvocationTerminal::CancelledBeforeCommit),
            InvocationProgress::Terminal(InvocationTerminal::RestartedBeforeCommit),
        ];
        for progress in progress {
            let bare_status = status(progress)?;
            let encoded = InvocationStatusCodec::encode(bare_status)?;
            assert_eq!(InvocationStatusCodec::decode(&encoded)?, bare_status);

            let reply = InvocationStatusReply::Retained(status(progress)?);
            let encoded = InvocationControlCodec::encode_status_reply(reply)?;
            assert_eq!(
                InvocationControlCodec::decode_status_reply(&encoded)?,
                reply
            );
        }
        Ok(())
    }

    #[test]
    fn status_and_cancel_outcomes_round_trip_without_ambiguous_tags()
    -> Result<(), Box<dyn std::error::Error>> {
        for reason in [
            InvocationUnavailable::Expired,
            InvocationUnavailable::UnknownInvocation,
            InvocationUnavailable::UnknownNamespace,
            InvocationUnavailable::Forbidden,
        ] {
            let status_reply = InvocationStatusReply::Unavailable(reason);
            let encoded = InvocationControlCodec::encode_status_reply(status_reply)?;
            assert_eq!(
                InvocationControlCodec::decode_status_reply(&encoded)?,
                status_reply
            );
        }
        assert_eq!(
            InvocationControlCodec::encode_status_reply(InvocationStatusReply::Unavailable(
                InvocationUnavailable::Expired,
            ))?,
            [0xA2, 0x00, 0x02, 0x01, 0x01]
        );

        let current = status(InvocationProgress::LogicalCommitted(state()?))?;
        for reply in [
            CancelInvocationReply::Cancelled(status(InvocationProgress::Terminal(
                InvocationTerminal::CancelledBeforeCommit,
            ))?),
            CancelInvocationReply::TooLate(current),
            CancelInvocationReply::AlreadyTerminal(status(InvocationProgress::Terminal(
                InvocationTerminal::RestartedBeforeCommit,
            ))?),
            CancelInvocationReply::Unavailable(InvocationUnavailable::Expired),
        ] {
            let encoded = InvocationControlCodec::encode_cancel_reply(reply)?;
            assert_eq!(
                InvocationControlCodec::decode_cancel_reply(&encoded)?,
                reply
            );
        }
        assert_eq!(
            InvocationControlCodec::encode_cancel_reply(CancelInvocationReply::Unavailable(
                InvocationUnavailable::Expired,
            ))?,
            [0xA2, 0x00, 0x04, 0x01, 0x01]
        );
        Ok(())
    }

    #[test]
    fn control_codec_rejects_invalid_shapes_and_codes() {
        assert!(matches!(
            InvocationControlCodec::decode_status_request(&[0xA2, 0x00, 0x80, 0x00, 0x80]),
            Err(CommandCodecError::WrongArrayLength { .. })
        ));
        assert!(matches!(
            InvocationControlCodec::decode_status_reply(&[0xA2, 0x00, 0x02, 0x01, 0x18, 0x63]),
            Err(CommandCodecError::UnknownInvocationUnavailable(99))
        ));
        assert!(matches!(
            InvocationControlCodec::decode_cancel_reply(&[0xA2, 0x00, 0x04, 0x01, 0x18, 0x63]),
            Err(CommandCodecError::UnknownInvocationUnavailable(99))
        ));
        assert!(matches!(
            InvocationControlCodec::decode_status_request(&[0xA0, 0x00]),
            Err(CommandCodecError::TrailingBytes)
        ));
    }

    proptest! {
        #[test]
        fn arbitrary_control_payloads_never_panic(bytes in prop::collection::vec(any::<u8>(), 0..20_000)) {
            let _ = InvocationStatusCodec::decode(&bytes);
            let _ = InvocationControlCodec::decode_status_request(&bytes);
            let _ = InvocationControlCodec::decode_status_reply(&bytes);
            let _ = InvocationControlCodec::decode_cancel_request(&bytes);
            let _ = InvocationControlCodec::decode_cancel_reply(&bytes);
        }
    }
}
