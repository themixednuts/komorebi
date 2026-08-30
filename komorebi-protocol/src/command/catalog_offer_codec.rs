use minicbor::Decoder;
use minicbor::Encoder;

use super::ActionAvailability;
use super::ActionOffer;
use super::ActionUnavailability;
use super::BoundedText;
use super::CommandCodecError;
use super::DynamicParameterChoices;
use super::ParameterId;
use super::catalog::MAX_BINDING_HINTS;
use super::catalog::MAX_DYNAMIC_CHOICE_GROUPS;
use super::catalog::MAX_DYNAMIC_CHOICES;
use super::codec::bounded_map;
use super::codec::decode_offer;
use super::codec::decode_scalar;
use super::codec::decode_state;
use super::codec::definite;
use super::codec::encode_offer;
use super::codec::encode_scalar;
use super::codec::encode_state;
use super::codec::required;
use super::codec::skip_bounded;
use super::codec::to_u64;
use super::codec::to_usize;
use super::codec::unique_key;

const AVAILABLE_TAG: u8 = 1;
const UNAVAILABLE_TAG: u8 = 2;

pub(super) fn encode_action_offer(
    encoder: &mut Encoder<Vec<u8>>,
    offer: &ActionOffer,
) -> Result<(), CommandCodecError> {
    let fields = if offer.current_value().is_some() {
        6
    } else {
        5
    };
    encoder.map(fields)?.u8(0)?;
    encode_offer(encoder, offer.reference())?;
    encoder.u8(1)?;
    encode_state(encoder, offer.state())?;
    encoder.u8(2)?;
    encode_availability(encoder, offer.availability())?;
    if let Some(current) = offer.current_value() {
        encoder.u8(3)?;
        encode_scalar(encoder, current)?;
    }
    encoder
        .u8(4)?
        .array(to_u64(offer.dynamic_choices().len())?)?;
    for choices in offer.dynamic_choices() {
        encoder
            .array(2)?
            .str(choices.parameter().as_str())?
            .array(to_u64(choices.choices().len())?)?;
        for choice in choices.choices() {
            encode_scalar(encoder, choice)?;
        }
    }
    encoder.u8(5)?.array(to_u64(offer.bindings().len())?)?;
    for binding in offer.bindings() {
        encoder.str(binding.as_str())?;
    }
    Ok(())
}

pub(super) fn decode_action_offer(
    decoder: &mut Decoder<'_>,
) -> Result<ActionOffer, CommandCodecError> {
    let count = bounded_map(decoder)?;
    let mut seen = [false; 256];
    let mut reference = None;
    let mut state = None;
    let mut availability = None;
    let mut current_value = None;
    let mut dynamic_choices = None;
    let mut bindings = None;
    for _ in 0..count {
        match unique_key(decoder, &mut seen)? {
            0 => reference = Some(decode_offer(decoder)?),
            1 => state = Some(decode_state(decoder)?),
            2 => availability = Some(decode_availability(decoder)?),
            3 => current_value = Some(decode_scalar(decoder)?),
            4 => dynamic_choices = Some(decode_dynamic_choices(decoder)?),
            5 => bindings = Some(decode_bindings(decoder)?),
            _ => skip_bounded(decoder, 0)?,
        }
    }
    Ok(ActionOffer::new(
        required(reference, 0)?,
        required(state, 1)?,
        required(availability, 2)?,
        current_value,
        required(dynamic_choices, 4)?,
        required(bindings, 5)?,
    )?)
}

fn encode_availability(
    encoder: &mut Encoder<Vec<u8>>,
    availability: ActionAvailability,
) -> Result<(), CommandCodecError> {
    match availability {
        ActionAvailability::Available => {
            encoder.array(1)?.u8(AVAILABLE_TAG)?;
        }
        ActionAvailability::Unavailable(reason) => {
            encoder.array(2)?.u8(UNAVAILABLE_TAG)?.u8(reason as u8)?;
        }
    }
    Ok(())
}

fn decode_availability(decoder: &mut Decoder<'_>) -> Result<ActionAvailability, CommandCodecError> {
    let length = definite(decoder.array()?)?;
    let tag = decoder.u8()?;
    match (tag, length) {
        (AVAILABLE_TAG, 1) => Ok(ActionAvailability::Available),
        (UNAVAILABLE_TAG, 2) => Ok(ActionAvailability::Unavailable(decode_unavailability(
            decoder.u8()?,
        )?)),
        (AVAILABLE_TAG | UNAVAILABLE_TAG, _) => {
            Err(CommandCodecError::WrongCatalogAvailabilityLength { tag, length })
        }
        _ => Err(CommandCodecError::UnknownCatalogAvailabilityTag(tag)),
    }
}

pub(super) fn decode_unavailability(value: u8) -> Result<ActionUnavailability, CommandCodecError> {
    match value {
        1 => Ok(ActionUnavailability::ManagerPaused),
        2 => Ok(ActionUnavailability::NoFocusedWindow),
        3 => Ok(ActionUnavailability::NoWindowInDirection),
        4 => Ok(ActionUnavailability::Unauthorized),
        5 => Ok(ActionUnavailability::UnknownWorkspace),
        _ => Err(CommandCodecError::UnknownActionUnavailability(value)),
    }
}

fn decode_dynamic_choices(
    decoder: &mut Decoder<'_>,
) -> Result<Vec<DynamicParameterChoices>, CommandCodecError> {
    let count = bounded_array(decoder, MAX_DYNAMIC_CHOICE_GROUPS)?;
    let mut groups = Vec::with_capacity(count);
    for _ in 0..count {
        let actual = definite(decoder.array()?)?;
        if actual != 2 {
            return Err(CommandCodecError::WrongArrayLength {
                expected: 2,
                actual,
            });
        }
        let parameter = ParameterId::parse(decoder.str()?)?;
        let choice_count = bounded_array(decoder, MAX_DYNAMIC_CHOICES)?;
        let mut choices = Vec::with_capacity(choice_count);
        for _ in 0..choice_count {
            choices.push(decode_scalar(decoder)?);
        }
        groups.push(DynamicParameterChoices::new(parameter, choices)?);
    }
    Ok(groups)
}

fn decode_bindings(decoder: &mut Decoder<'_>) -> Result<Vec<BoundedText>, CommandCodecError> {
    let count = bounded_array(decoder, MAX_BINDING_HINTS)?;
    let mut bindings = Vec::with_capacity(count);
    for _ in 0..count {
        bindings.push(BoundedText::new(decoder.str()?)?);
    }
    Ok(bindings)
}

fn bounded_array(decoder: &mut Decoder<'_>, maximum: usize) -> Result<usize, CommandCodecError> {
    let count = to_usize(definite(decoder.array()?)?)?;
    if count > maximum {
        Err(CommandCodecError::CatalogCollectionTooLarge {
            actual: count,
            maximum,
        })
    } else {
        Ok(count)
    }
}
