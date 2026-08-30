use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::num::NonZeroU16;
use std::num::NonZeroU64;

use minicbor::Encoder;
use proptest::prelude::*;

use super::*;
use crate::InvocationId;
use crate::InvocationNamespaceId;
use crate::InvocationSequence;
use crate::ManagerEpoch;

fn invocation(arguments: ActionArguments) -> Result<ActionInvocation, Box<dyn std::error::Error>> {
    let epoch = ManagerEpoch::new([1; 16])?;
    let action = ActionKey::new(
        ActionId::parse("focus-window")?,
        ActionSchemaVersion::new(NonZeroU16::MIN),
    );
    let catalog = CatalogStamp::new(
        epoch,
        Revision::try_from(1)?,
        Revision::try_from(2)?,
        Revision::try_from(3)?,
    );
    Ok(ActionInvocation::new(
        InvocationId::new(
            InvocationNamespaceId::new([2; 16])?,
            InvocationSequence::new(NonZeroU64::new(7).ok_or("zero sequence")?),
        ),
        OfferRef::new(action, ActionContractFingerprint::new([3; 32]), catalog),
        StateStamp::new(epoch, Revision::try_from(4)?),
        arguments,
        Some(ConfirmationChallengeId::new([4; 16])?),
    ))
}

fn all_arguments() -> Result<ActionArguments, Box<dyn std::error::Error>> {
    let values = BTreeMap::from([
        (
            ParameterId::parse("bool")?,
            ActionArgument::Scalar(ArgumentScalar::Bool(true)),
        ),
        (
            ParameterId::parse("choice")?,
            ActionArgument::Scalar(ArgumentScalar::Choice(ChoiceId::parse("left")?)),
        ),
        (
            ParameterId::parse("color")?,
            ActionArgument::Scalar(ArgumentScalar::Color(Color::new(1, 2, 3, 4))),
        ),
        (
            ParameterId::parse("decimal")?,
            ActionArgument::Scalar(ArgumentScalar::Decimal(FixedDecimal::new(-125, 2)?)),
        ),
        (
            ParameterId::parse("entity")?,
            ActionArgument::Scalar(ArgumentScalar::Entity(EntityReference::new(
                EntityKind::parse("workspace")?,
                EntityId::parse("chat")?,
            ))),
        ),
        (
            ParameterId::parse("list")?,
            ActionArgument::Scalars(ArgumentScalars::new(
                vec![ArgumentScalar::Signed(-1), ArgumentScalar::Unsigned(2)].into_boxed_slice(),
            )?),
        ),
        (
            ParameterId::parse("path")?,
            ActionArgument::Scalar(ArgumentScalar::WindowsPath(WindowsPathInput::new(
                vec![
                    u16::from(b'C'),
                    u16::from(b':'),
                    u16::from(b'\\'),
                    0xD800,
                    u16::from(b'x'),
                ]
                .into_boxed_slice(),
            )?)),
        ),
        (
            ParameterId::parse("selector")?,
            ActionArgument::Scalar(ArgumentScalar::Selector(SelectorId::parse(
                "focused-at-execution",
            )?)),
        ),
        (
            ParameterId::parse("signed")?,
            ActionArgument::Scalar(ArgumentScalar::Signed(i64::MIN)),
        ),
        (
            ParameterId::parse("text")?,
            ActionArgument::Scalar(ArgumentScalar::Text(BoundedText::new("日本語")?)),
        ),
        (
            ParameterId::parse("unit")?,
            ActionArgument::Scalar(ArgumentScalar::Unit(UnitValue::new(Unit::Pixels, -20))),
        ),
        (
            ParameterId::parse("unsigned")?,
            ActionArgument::Scalar(ArgumentScalar::Unsigned(u64::MAX)),
        ),
    ]);
    Ok(ActionArguments::new(values)?)
}

#[test]
fn closed_arguments_and_wtf16_path_round_trip_canonically() -> Result<(), Box<dyn std::error::Error>>
{
    let expected = invocation(all_arguments()?)?;
    let encoded = ActionInvocationCodec::encode(&expected)?;
    let decoded = ActionInvocationCodec::decode(&encoded)?;

    assert_eq!(decoded, expected);
    assert_eq!(ActionInvocationCodec::encode(&decoded)?, encoded);
    let path = decoded
        .arguments()
        .values()
        .get(&ParameterId::parse("path")?)
        .ok_or("path argument missing")?;
    let ActionArgument::Scalar(ArgumentScalar::WindowsPath(path)) = path else {
        return Err("path argument changed type".into());
    };
    assert_eq!(path.units()[3], 0xD800);
    Ok(())
}

#[test]
fn canonical_digest_is_stable_and_covers_arguments() -> Result<(), Box<dyn std::error::Error>> {
    let original = invocation(ActionArguments::new(BTreeMap::from([(
        ParameterId::parse("enabled")?,
        ActionArgument::Scalar(ArgumentScalar::Bool(true)),
    )]))?)?;
    let changed = invocation(ActionArguments::new(BTreeMap::from([(
        ParameterId::parse("enabled")?,
        ActionArgument::Scalar(ArgumentScalar::Bool(false)),
    )]))?)?;

    let digest = ActionInvocationCodec::digest(&original)?;
    assert_eq!(ActionInvocationCodec::digest(&original)?, digest);
    assert_ne!(ActionInvocationCodec::digest(&changed)?, digest);
    Ok(())
}

#[test]
fn bounded_unknown_top_level_fields_are_ignored() -> Result<(), Box<dyn std::error::Error>> {
    let expected = invocation(ActionArguments::default())?;
    let mut encoded = ActionInvocationCodec::encode(&expected)?;
    let mut canonical_hex = String::with_capacity(encoded.len() * 2);
    for byte in &encoded {
        write!(canonical_hex, "{byte:02x}")?;
    }
    assert_eq!(
        canonical_hex,
        "a5008250020202020202020202020202020202020701a300a2006c666f6375732d77696e646f770101015820030303030303030303030303030303030303030303030303030303030303030302a400500101010101010101010101010101010101010202030302a200500101010101010101010101010101010101040380045004040404040404040404040404040404"
    );
    assert_eq!(encoded[0], 0xA5);
    encoded[0] = 0xA6;
    encoded.extend_from_slice(&[0x18, 0xFF, 0x81, 0x01]);

    assert_eq!(ActionInvocationCodec::decode(&encoded)?, expected);
    Ok(())
}

#[test]
fn duplicate_fields_and_oversized_payloads_fail_closed() -> Result<(), Box<dyn std::error::Error>> {
    let mut encoder = Encoder::new(Vec::new());
    encoder
        .map(2)?
        .u8(0)?
        .array(2)?
        .bytes(&[2; 16])?
        .u64(1)?
        .u8(0)?;
    assert!(matches!(
        ActionInvocationCodec::decode(&encoder.into_writer()),
        Err(ActionInvocationCodecError::DuplicateKey(0))
    ));

    assert!(matches!(
        ActionInvocationCodec::decode(&vec![0; 16 * 1024 + 1]),
        Err(ActionInvocationCodecError::PayloadTooLarge(_))
    ));
    Ok(())
}

#[test]
fn primitive_validation_excludes_ambiguous_values() {
    assert!(ActionId::parse("Focus_Window").is_err());
    assert_eq!(
        WindowsPathInput::new([u16::from(b'C'), 0, u16::from(b'x')]),
        Err(ArgumentError::PathContainsNul)
    );
    assert_eq!(
        ArgumentScalars::new(Vec::<ArgumentScalar>::new().into_boxed_slice()),
        Err(ArgumentError::EmptyList)
    );
    assert_eq!(
        Revision::try_from(0),
        Err(ActionContractError::ZeroRevision)
    );
}

proptest! {
    #[test]
    fn arbitrary_invocation_payloads_never_panic(bytes in prop::collection::vec(any::<u8>(), 0..=20_000)) {
        let _ = ActionInvocationCodec::decode(&bytes);
    }
}
