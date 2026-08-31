use minicbor::Decoder;
use minicbor::Encoder;

use super::ActionId;
use super::ActionIntent;
use super::CommandCodecError;
use super::codec::MAX_COMMAND_PAYLOAD_BYTES;
use super::codec::decode_arguments;
use super::codec::definite;
use super::codec::encode_arguments;

/// Canonical codec for an unbound action intent.
#[derive(Clone, Copy, Debug, Default)]
pub struct ActionIntentCodec;

impl ActionIntentCodec {
    /// Encodes one action intent into a bounded canonical payload.
    ///
    /// # Errors
    ///
    /// Returns [`CommandCodecError`] when an argument cannot be encoded or the
    /// canonical payload exceeds the command boundary.
    pub fn encode(intent: &ActionIntent) -> Result<Vec<u8>, CommandCodecError> {
        let mut encoder = Encoder::new(Vec::with_capacity(256));
        encoder.array(2)?.str(intent.action().as_str())?;
        encode_arguments(&mut encoder, intent.arguments())?;
        let payload = encoder.into_writer();
        if payload.len() > MAX_COMMAND_PAYLOAD_BYTES {
            Err(CommandCodecError::PayloadTooLarge(payload.len()))
        } else {
            Ok(payload)
        }
    }

    /// Decodes one exact canonical action-intent payload.
    ///
    /// # Errors
    ///
    /// Returns [`CommandCodecError`] for malformed, noncanonical, oversized,
    /// or trailing input.
    pub fn decode(bytes: &[u8]) -> Result<ActionIntent, CommandCodecError> {
        if bytes.len() > MAX_COMMAND_PAYLOAD_BYTES {
            return Err(CommandCodecError::PayloadTooLarge(bytes.len()));
        }
        let mut decoder = Decoder::new(bytes);
        let length = definite(decoder.array()?)?;
        if length != 2 {
            return Err(CommandCodecError::WrongArrayLength {
                expected: 2,
                actual: length,
            });
        }
        let action = ActionId::parse(decoder.str()?)?;
        let arguments = decode_arguments(&mut decoder)?;
        if decoder.position() != bytes.len() {
            return Err(CommandCodecError::TrailingBytes);
        }
        Ok(ActionIntent::new(action, arguments))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::ActionArgument;
    use crate::ActionArguments;
    use crate::ArgumentScalar;
    use crate::ChoiceId;
    use crate::ParameterId;
    use crate::WindowsPathInput;

    #[test]
    fn action_intent_round_trip_preserves_typed_values_and_wtf16_paths()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut arguments = BTreeMap::new();
        arguments.insert(
            ParameterId::parse("direction")?,
            ActionArgument::Scalar(ArgumentScalar::Choice(ChoiceId::parse("left")?)),
        );
        arguments.insert(
            ParameterId::parse("path")?,
            ActionArgument::Scalar(ArgumentScalar::WindowsPath(WindowsPathInput::new(
                vec![b'C'.into(), b':'.into(), b'\\'.into(), 0xd800].into_boxed_slice(),
            )?)),
        );
        let intent = ActionIntent::new(
            ActionId::parse("open-at-path")?,
            ActionArguments::new(arguments)?,
        );

        let encoded = ActionIntentCodec::encode(&intent)?;

        assert_eq!(ActionIntentCodec::decode(&encoded)?, intent);
        Ok(())
    }

    #[test]
    fn action_intent_rejects_trailing_bytes() -> Result<(), Box<dyn std::error::Error>> {
        let intent =
            ActionIntent::new(ActionId::parse("toggle-pause")?, ActionArguments::default());
        let mut encoded = ActionIntentCodec::encode(&intent)?;
        encoded.push(0);

        assert!(matches!(
            ActionIntentCodec::decode(&encoded),
            Err(CommandCodecError::TrailingBytes)
        ));
        Ok(())
    }
}
