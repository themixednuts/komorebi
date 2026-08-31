use minicbor::Decoder;
use minicbor::Encoder;
use sha2::Digest;
use sha2::Sha256;

use super::ActionCategory;
use super::ActionContractFingerprint;
use super::ActionDefinition;
use super::ActionDefinitionSpec;
use super::ActionParameter;
use super::ArgumentCardinality;
use super::BoundedText;
use super::CommandCodecError;
use super::ConfirmationPolicy;
use super::ParameterDomain;
use super::ParameterId;
use super::PermittedUse;
use super::UndoPolicy;
use super::catalog::MAX_DEFINITION_KEYWORDS;
use super::catalog::MAX_DEFINITION_PARAMETERS;
use super::codec::bounded_map;
use super::codec::decode_action_key;
use super::codec::definite;
use super::codec::encode_action_key;
use super::codec::required;
use super::codec::skip_bounded;
use super::codec::to_u64;
use super::codec::to_usize;
use super::codec::unique_key;

pub(super) fn fingerprint(
    definition: &ActionDefinition,
) -> Result<ActionContractFingerprint, CommandCodecError> {
    let mut encoder = Encoder::new(Vec::with_capacity(256));
    encode_definition(&mut encoder, definition)?;
    Ok(ActionContractFingerprint::new(
        Sha256::digest(encoder.into_writer()).into(),
    ))
}

pub(super) fn encode_definition(
    encoder: &mut Encoder<Vec<u8>>,
    definition: &ActionDefinition,
) -> Result<(), CommandCodecError> {
    encoder.map(9)?.u8(0)?;
    encode_action_key(encoder, definition.key())?;
    encoder
        .u8(1)?
        .u8(definition.category() as u8)?
        .u8(2)?
        .str(definition.title().as_str())?
        .u8(3)?
        .str(definition.description().as_str())?
        .u8(4)?
        .array(to_u64(definition.keywords().len())?)?;
    for keyword in definition.keywords() {
        encoder.str(keyword.as_str())?;
    }
    encoder
        .u8(5)?
        .array(to_u64(definition.parameters().len())?)?;
    for parameter in definition.parameters() {
        encoder
            .array(3)?
            .str(parameter.id().as_str())?
            .u8(parameter.domain() as u8)?
            .u8(parameter.cardinality() as u8)?;
    }
    encoder
        .u8(6)?
        .array(to_u64(definition.permitted_uses().len())?)?;
    for permitted in definition.permitted_uses() {
        encoder.u8(*permitted as u8)?;
    }
    encoder
        .u8(7)?
        .u8(definition.confirmation() as u8)?
        .u8(8)?
        .u8(definition.undo() as u8)?;
    Ok(())
}

pub(super) fn decode_definition(
    decoder: &mut Decoder<'_>,
) -> Result<ActionDefinition, CommandCodecError> {
    let count = bounded_map(decoder)?;
    let mut seen = [false; 256];
    let mut key = None;
    let mut category = None;
    let mut title = None;
    let mut description = None;
    let mut keywords = None;
    let mut parameters = None;
    let mut permitted_uses = None;
    let mut confirmation = None;
    let mut undo = None;
    for _ in 0..count {
        match unique_key(decoder, &mut seen)? {
            0 => key = Some(decode_action_key(decoder)?),
            1 => category = Some(decode_category(decoder.u8()?)?),
            2 => title = Some(BoundedText::new(decoder.str()?)?),
            3 => description = Some(BoundedText::new(decoder.str()?)?),
            4 => keywords = Some(decode_keywords(decoder)?),
            5 => parameters = Some(decode_parameters(decoder)?),
            6 => permitted_uses = Some(decode_permitted_uses(decoder)?),
            7 => confirmation = Some(decode_confirmation(decoder.u8()?)?),
            8 => undo = Some(decode_undo(decoder.u8()?)?),
            _ => skip_bounded(decoder, 0)?,
        }
    }
    Ok(ActionDefinition::new(ActionDefinitionSpec {
        key: required(key, 0)?,
        category: required(category, 1)?,
        title: required(title, 2)?,
        description: required(description, 3)?,
        keywords: required(keywords, 4)?,
        parameters: required(parameters, 5)?,
        permitted_uses: required(permitted_uses, 6)?,
        confirmation: required(confirmation, 7)?,
        undo: required(undo, 8)?,
    })?)
}

fn decode_keywords(decoder: &mut Decoder<'_>) -> Result<Vec<BoundedText>, CommandCodecError> {
    let count = bounded_count(decoder, MAX_DEFINITION_KEYWORDS)?;
    let mut keywords = Vec::with_capacity(count);
    for _ in 0..count {
        keywords.push(BoundedText::new(decoder.str()?)?);
    }
    Ok(keywords)
}

fn decode_parameters(decoder: &mut Decoder<'_>) -> Result<Vec<ActionParameter>, CommandCodecError> {
    let count = bounded_count(decoder, MAX_DEFINITION_PARAMETERS)?;
    let mut parameters = Vec::with_capacity(count);
    for _ in 0..count {
        let actual = definite(decoder.array()?)?;
        if actual != 3 {
            return Err(CommandCodecError::WrongArrayLength {
                expected: 3,
                actual,
            });
        }
        parameters.push(ActionParameter::new(
            ParameterId::parse(decoder.str()?)?,
            decode_domain(decoder.u8()?)?,
            decode_cardinality(decoder.u8()?)?,
        ));
    }
    Ok(parameters)
}

fn decode_cardinality(value: u8) -> Result<ArgumentCardinality, CommandCodecError> {
    match value {
        1 => Ok(ArgumentCardinality::RequiredScalar),
        2 => Ok(ArgumentCardinality::RequiredList),
        3 => Ok(ArgumentCardinality::OptionalScalar),
        4 => Ok(ArgumentCardinality::OptionalList),
        _ => Err(CommandCodecError::UnknownArgumentCardinality(value)),
    }
}

fn decode_permitted_uses(
    decoder: &mut Decoder<'_>,
) -> Result<Vec<PermittedUse>, CommandCodecError> {
    let count = bounded_count(decoder, 2)?;
    let mut permitted = Vec::with_capacity(count);
    for _ in 0..count {
        permitted.push(match decoder.u8()? {
            1 => PermittedUse::Interactive,
            2 => PermittedUse::Automation,
            value => return Err(CommandCodecError::UnknownPermittedUse(value)),
        });
    }
    Ok(permitted)
}

fn bounded_count(decoder: &mut Decoder<'_>, maximum: usize) -> Result<usize, CommandCodecError> {
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

fn decode_category(value: u8) -> Result<ActionCategory, CommandCodecError> {
    match value {
        1 => Ok(ActionCategory::Window),
        2 => Ok(ActionCategory::Workspace),
        3 => Ok(ActionCategory::Configuration),
        _ => Err(CommandCodecError::UnknownActionCategory(value)),
    }
}

fn decode_confirmation(value: u8) -> Result<ConfirmationPolicy, CommandCodecError> {
    match value {
        1 => Ok(ConfirmationPolicy::None),
        _ => Err(CommandCodecError::UnknownConfirmationPolicy(value)),
    }
}

fn decode_undo(value: u8) -> Result<UndoPolicy, CommandCodecError> {
    match value {
        1 => Ok(UndoPolicy::None),
        2 => Ok(UndoPolicy::PriorManagerIntent),
        3 => Ok(UndoPolicy::ExactCapturedState),
        _ => Err(CommandCodecError::UnknownUndoPolicy(value)),
    }
}

fn decode_domain(value: u8) -> Result<ParameterDomain, CommandCodecError> {
    match value {
        1 => Ok(ParameterDomain::Direction),
        2 => Ok(ParameterDomain::Axis),
        3 => Ok(ParameterDomain::Pixels),
        4 => Ok(ParameterDomain::WorkspaceSelector),
        5 => Ok(ParameterDomain::WindowSelector),
        6 => Ok(ParameterDomain::Layout),
        7 => Ok(ParameterDomain::Cycle),
        8 => Ok(ParameterDomain::Index),
        9 => Ok(ParameterDomain::Sizing),
        10 => Ok(ParameterDomain::Adjustment),
        11 => Ok(ParameterDomain::Flag),
        12 => Ok(ParameterDomain::Size),
        13 => Ok(ParameterDomain::Count),
        14 => Ok(ParameterDomain::Columns),
        15 => Ok(ParameterDomain::Name),
        16 => Ok(ParameterDomain::Path),
        17 => Ok(ParameterDomain::Behaviour),
        18 => Ok(ParameterDomain::Implementation),
        19 => Ok(ParameterDomain::Executable),
        20 => Ok(ParameterDomain::Identifier),
        21 => Ok(ParameterDomain::Ratios),
        22 => Ok(ParameterDomain::AtCount),
        23 => Ok(ParameterDomain::ResizeStep),
        24 => Ok(ParameterDomain::Alpha),
        25 => Ok(ParameterDomain::WindowKind),
        26 => Ok(ParameterDomain::ColourChannel),
        27 => Ok(ParameterDomain::BorderWidth),
        28 => Ok(ParameterDomain::BorderOffset),
        29 => Ok(ParameterDomain::BorderStyle),
        30 => Ok(ParameterDomain::BorderImplementation),
        31 => Ok(ParameterDomain::StackbarMode),
        32 => Ok(ParameterDomain::StackbarLabel),
        33 => Ok(ParameterDomain::StackbarHeight),
        34 => Ok(ParameterDomain::StackbarTabWidth),
        35 => Ok(ParameterDomain::StackbarFontSize),
        36 => Ok(ParameterDomain::StackbarFontFamily),
        37 => Ok(ParameterDomain::AnimationPrefix),
        38 => Ok(ParameterDomain::AnimationDuration),
        39 => Ok(ParameterDomain::AnimationFps),
        40 => Ok(ParameterDomain::AnimationStyle),
        41 => Ok(ParameterDomain::WorkAreaOffset),
        42 => Ok(ParameterDomain::CursorWarpPolicy),
        _ => Err(CommandCodecError::UnknownParameterDomain(value)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cardinality_codes_are_closed() {
        assert!(matches!(
            decode_cardinality(1),
            Ok(ArgumentCardinality::RequiredScalar)
        ));
        assert!(matches!(
            decode_cardinality(u8::MAX),
            Err(CommandCodecError::UnknownArgumentCardinality(u8::MAX))
        ));
    }
}
