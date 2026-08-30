use minicbor::Decoder;
use minicbor::Encoder;

use super::CommandCodecError;
use super::InvocationRejection;
use super::InvocationSubmissionReply;
use super::catalog_offer_codec::decode_unavailability;
use super::codec::bounded_encoded;
use super::codec::bounded_map;
use super::codec::decode_catalog;
use super::codec::decode_state;
use super::codec::definite;
use super::codec::encode_catalog;
use super::codec::encode_state;
use super::codec::ensure_command_bound;
use super::codec::require_eof;
use super::codec::required;
use super::codec::skip_bounded;
use super::codec::unique_key;
use super::control_codec::decode_status;
use super::control_codec::encode_status;

const ACCEPTED_TAG: u8 = 1;
const RETAINED_TAG: u8 = 2;
const REJECTED_TAG: u8 = 3;

#[derive(Clone, Copy, Debug, Default)]
pub struct InvocationSubmissionCodec;

impl InvocationSubmissionCodec {
    /// Encodes one canonical invocation submission outcome.
    ///
    /// # Errors
    ///
    /// Returns [`CommandCodecError`] when CBOR encoding fails.
    pub fn encode(reply: InvocationSubmissionReply) -> Result<Vec<u8>, CommandCodecError> {
        let mut encoder = Encoder::new(Vec::with_capacity(128));
        encoder.map(2)?.u8(0)?;
        match reply {
            InvocationSubmissionReply::Accepted(status) => {
                encoder.u8(ACCEPTED_TAG)?.u8(1)?;
                encode_status(&mut encoder, status)?;
            }
            InvocationSubmissionReply::Retained(status) => {
                encoder.u8(RETAINED_TAG)?.u8(1)?;
                encode_status(&mut encoder, status)?;
            }
            InvocationSubmissionReply::Rejected(rejection) => {
                encoder.u8(REJECTED_TAG)?.u8(1)?;
                encode_rejection(&mut encoder, rejection)?;
            }
        }
        bounded_encoded(encoder.into_writer())
    }

    /// Decodes one strict bounded invocation submission outcome.
    ///
    /// # Errors
    ///
    /// Returns [`CommandCodecError`] for malformed, duplicate, missing,
    /// oversized, indefinite, unknown, or trailing input.
    pub fn decode(bytes: &[u8]) -> Result<InvocationSubmissionReply, CommandCodecError> {
        ensure_command_bound(bytes)?;
        let mut decoder = Decoder::new(bytes);
        let count = bounded_map(&mut decoder)?;
        let mut seen = [false; 256];
        let mut tag = None;
        let mut reply = None;
        for _ in 0..count {
            match unique_key(&mut decoder, &mut seen)? {
                0 => tag = Some(decoder.u8()?),
                1 => {
                    reply = Some(match required(tag, 0)? {
                        ACCEPTED_TAG => {
                            InvocationSubmissionReply::Accepted(decode_status(&mut decoder)?)
                        }
                        RETAINED_TAG => {
                            InvocationSubmissionReply::Retained(decode_status(&mut decoder)?)
                        }
                        REJECTED_TAG => {
                            InvocationSubmissionReply::Rejected(decode_rejection(&mut decoder)?)
                        }
                        unknown => {
                            return Err(CommandCodecError::UnknownSubmissionReplyTag(unknown));
                        }
                    });
                }
                _ => skip_bounded(&mut decoder, 0)?,
            }
        }
        require_eof(&decoder, bytes)?;
        required(reply, 1)
    }
}

fn encode_rejection(
    encoder: &mut Encoder<Vec<u8>>,
    rejection: InvocationRejection,
) -> Result<(), CommandCodecError> {
    use InvocationRejection as R;
    match rejection {
        R::StaleState { current } => {
            encoder.array(2)?.u8(8)?;
            encode_state(encoder, current)?;
        }
        R::StaleCatalog { current } => {
            encoder.array(2)?.u8(9)?;
            encode_catalog(encoder, current)?;
        }
        R::Unavailable(reason) => {
            encoder.array(2)?.u8(12)?.u8(reason as u8)?;
        }
        simple => {
            encoder.array(1)?.u8(simple_rejection_code(simple))?;
        }
    }
    Ok(())
}

const fn simple_rejection_code(rejection: InvocationRejection) -> u8 {
    use InvocationRejection as R;
    match rejection {
        R::Unauthorized => 1,
        R::IdempotencyConflict => 2,
        R::InvocationExpired => 3,
        R::InvocationNotLeased => 4,
        R::UnknownNamespace => 5,
        R::CapacityFull => 6,
        R::StaleEpoch => 7,
        R::StaleOffer => 10,
        R::InvalidArguments => 11,
        R::ConfirmationRequired => 13,
        R::ConfirmationUnavailable => 14,
        R::StaleState { .. } | R::StaleCatalog { .. } | R::Unavailable(_) => unreachable!(),
    }
}

fn decode_rejection(decoder: &mut Decoder<'_>) -> Result<InvocationRejection, CommandCodecError> {
    use InvocationRejection as R;
    let length = definite(decoder.array()?)?;
    let tag = decoder.u8()?;
    let rejection = match (tag, length) {
        (1, 1) => R::Unauthorized,
        (2, 1) => R::IdempotencyConflict,
        (3, 1) => R::InvocationExpired,
        (4, 1) => R::InvocationNotLeased,
        (5, 1) => R::UnknownNamespace,
        (6, 1) => R::CapacityFull,
        (7, 1) => R::StaleEpoch,
        (8, 2) => R::StaleState {
            current: decode_state(decoder)?,
        },
        (9, 2) => R::StaleCatalog {
            current: decode_catalog(decoder)?,
        },
        (10, 1) => R::StaleOffer,
        (11, 1) => R::InvalidArguments,
        (12, 2) => R::Unavailable(decode_unavailability(decoder.u8()?)?),
        (13, 1) => R::ConfirmationRequired,
        (14, 1) => R::ConfirmationUnavailable,
        (1..=14, _) => {
            return Err(CommandCodecError::WrongInvocationRejectionLength { tag, length });
        }
        _ => return Err(CommandCodecError::UnknownInvocationRejection(tag)),
    };
    Ok(rejection)
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::InvocationDigest;
    use crate::InvocationId;
    use crate::InvocationNamespaceId;
    use crate::InvocationSequence;
    use crate::ManagerEpoch;
    use crate::Revision;
    use crate::command::ActionUnavailability;
    use crate::command::InvocationProgress;
    use crate::command::InvocationStatus;

    fn state(revision: u64) -> Result<super::super::StateStamp, Box<dyn std::error::Error>> {
        Ok(super::super::StateStamp::new(
            ManagerEpoch::new([1; 16])?,
            Revision::try_from(revision)?,
        ))
    }

    fn status() -> Result<InvocationStatus, Box<dyn std::error::Error>> {
        Ok(InvocationStatus::new(
            InvocationId::new(
                InvocationNamespaceId::new([2; 16])?,
                InvocationSequence::try_from(3)?,
            ),
            InvocationDigest::new([4; 32])?,
            InvocationProgress::Reserved,
        ))
    }

    #[test]
    fn every_submission_outcome_round_trips_canonically() -> Result<(), Box<dyn std::error::Error>>
    {
        let current_state = state(5)?;
        let current_catalog = super::super::CatalogStamp::new(
            current_state.epoch(),
            Revision::FIRST,
            current_state.revision(),
            Revision::FIRST,
        );
        let rejections = [
            InvocationRejection::Unauthorized,
            InvocationRejection::IdempotencyConflict,
            InvocationRejection::InvocationExpired,
            InvocationRejection::InvocationNotLeased,
            InvocationRejection::UnknownNamespace,
            InvocationRejection::CapacityFull,
            InvocationRejection::StaleEpoch,
            InvocationRejection::StaleState {
                current: current_state,
            },
            InvocationRejection::StaleCatalog {
                current: current_catalog,
            },
            InvocationRejection::StaleOffer,
            InvocationRejection::InvalidArguments,
            InvocationRejection::Unavailable(ActionUnavailability::ManagerPaused),
            InvocationRejection::ConfirmationRequired,
            InvocationRejection::ConfirmationUnavailable,
        ];
        let mut replies = vec![
            InvocationSubmissionReply::Accepted(status()?),
            InvocationSubmissionReply::Retained(status()?),
        ];
        replies.extend(
            rejections
                .into_iter()
                .map(InvocationSubmissionReply::Rejected),
        );

        for reply in replies {
            let encoded = InvocationSubmissionCodec::encode(reply)?;
            assert_eq!(InvocationSubmissionCodec::decode(&encoded)?, reply);
        }
        Ok(())
    }

    #[test]
    fn rejection_shapes_fail_closed() {
        let wrong_length = [0xa2, 0x00, REJECTED_TAG, 0x01, 0x82, 0x01, 0x00];
        assert!(matches!(
            InvocationSubmissionCodec::decode(&wrong_length),
            Err(CommandCodecError::WrongInvocationRejectionLength { tag: 1, length: 2 })
        ));
        let unknown = [0xa2, 0x00, REJECTED_TAG, 0x01, 0x81, 0x18, 0xff];
        assert!(matches!(
            InvocationSubmissionCodec::decode(&unknown),
            Err(CommandCodecError::UnknownInvocationRejection(u8::MAX))
        ));
    }

    proptest! {
        #[test]
        fn arbitrary_submission_bytes_never_panic(bytes in prop::collection::vec(any::<u8>(), 0..20_000)) {
            let _ = InvocationSubmissionCodec::decode(&bytes);
        }
    }
}
