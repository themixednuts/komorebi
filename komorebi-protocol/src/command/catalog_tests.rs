use std::num::NonZeroU16;

use proptest::prelude::*;

use super::*;
use crate::ManagerEpoch;

fn epoch() -> Result<ManagerEpoch, crate::IdentifierError> {
    ManagerEpoch::new([1; 16])
}

fn state() -> Result<StateStamp, Box<dyn std::error::Error>> {
    Ok(StateStamp::new(epoch()?, Revision::try_from(7)?))
}

fn stamp() -> Result<CatalogStamp, Box<dyn std::error::Error>> {
    Ok(CatalogStamp::new(
        epoch()?,
        Revision::try_from(2)?,
        Revision::try_from(7)?,
        Revision::FIRST,
    ))
}

fn definition() -> Result<ActionDefinition, Box<dyn std::error::Error>> {
    Ok(ActionDefinition::new(ActionDefinitionSpec {
        key: ActionKey::new(
            ActionId::parse("focus-window")?,
            ActionSchemaVersion::new(NonZeroU16::MIN),
        ),
        category: ActionCategory::Window,
        title: BoundedText::new("Focus window")?,
        description: BoundedText::new("Focus the neighboring window")?,
        keywords: vec![BoundedText::new("focus")?, BoundedText::new("window")?],
        parameters: vec![ActionParameter::new(
            ParameterId::parse("direction")?,
            ParameterDomain::Direction,
        )],
        permitted_uses: vec![PermittedUse::Automation, PermittedUse::Interactive],
        confirmation: ConfirmationPolicy::None,
        undo: UndoPolicy::None,
    })?)
}

fn snapshot() -> Result<CatalogSnapshot, Box<dyn std::error::Error>> {
    let definition = definition()?;
    let fingerprint = CatalogCodec::definition_fingerprint(&definition)?;
    let offer = ActionOffer::new(
        OfferRef::new(definition.key().clone(), fingerprint, stamp()?),
        state()?,
        ActionAvailability::Available,
        Some(ArgumentScalar::Choice(ChoiceId::parse("left")?)),
        vec![DynamicParameterChoices::new(
            ParameterId::parse("direction")?,
            vec![
                ArgumentScalar::Choice(ChoiceId::parse("left")?),
                ArgumentScalar::Choice(ChoiceId::parse("right")?),
            ],
        )?],
        vec![BoundedText::new("alt+h")?],
    )?;
    Ok(CatalogSnapshot::new(
        stamp()?,
        state()?,
        vec![definition],
        vec![offer],
    )?)
}

#[test]
fn query_has_a_byte_exact_cache_stamp_fixture() -> Result<(), Box<dyn std::error::Error>> {
    let query = CatalogQuery::new(Some(stamp()?));
    let encoded = CatalogCodec::encode_query(query)?;
    let mut expected = vec![0xA1, 0x00, 0xA4, 0x00, 0x50];
    expected.extend_from_slice(&[1; 16]);
    expected.extend_from_slice(&[0x01, 0x02, 0x02, 0x07, 0x03, 0x01]);
    assert_eq!(encoded, expected);
    assert_eq!(CatalogCodec::decode_query(&encoded)?, query);
    Ok(())
}

#[test]
fn full_catalog_round_trip_preserves_definition_offer_and_dynamic_values()
-> Result<(), Box<dyn std::error::Error>> {
    let reply = CatalogReply::Snapshot(snapshot()?);
    let encoded = CatalogCodec::encode_reply(&reply)?;
    assert_eq!(CatalogCodec::decode_reply(&encoded)?, reply);
    Ok(())
}

#[test]
fn not_modified_round_trip_preserves_the_exact_catalog_stamp()
-> Result<(), Box<dyn std::error::Error>> {
    let reply = CatalogReply::NotModified(stamp()?);
    let encoded = CatalogCodec::encode_reply(&reply)?;
    assert_eq!(CatalogCodec::decode_reply(&encoded)?, reply);
    Ok(())
}

#[test]
fn mismatched_contract_fingerprint_never_leaves_the_catalog_boundary()
-> Result<(), Box<dyn std::error::Error>> {
    let definition = definition()?;
    let offer = ActionOffer::new(
        OfferRef::new(
            definition.key().clone(),
            ActionContractFingerprint::new([9; 32]),
            stamp()?,
        ),
        state()?,
        ActionAvailability::Available,
        None,
        Vec::new(),
        Vec::new(),
    )?;
    let reply = CatalogReply::Snapshot(CatalogSnapshot::new(
        stamp()?,
        state()?,
        vec![definition],
        vec![offer],
    )?);
    assert!(matches!(
        CatalogCodec::encode_reply(&reply),
        Err(CommandCodecError::DefinitionFingerprintMismatch)
    ));
    Ok(())
}

#[test]
fn snapshot_constructor_sorts_definitions_and_offers_together()
-> Result<(), Box<dyn std::error::Error>> {
    let first = definition()?;
    let second = ActionDefinition::new(ActionDefinitionSpec {
        key: ActionKey::new(
            ActionId::parse("close-window")?,
            ActionSchemaVersion::new(NonZeroU16::MIN),
        ),
        category: ActionCategory::Window,
        title: BoundedText::new("Close window")?,
        description: BoundedText::new("Close the focused window")?,
        keywords: Vec::new(),
        parameters: Vec::new(),
        permitted_uses: vec![PermittedUse::Interactive],
        confirmation: ConfirmationPolicy::None,
        undo: UndoPolicy::None,
    })?;
    let first_offer = offer_for(&first)?;
    let second_offer = offer_for(&second)?;
    let snapshot = CatalogSnapshot::new(
        stamp()?,
        state()?,
        vec![first, second],
        vec![first_offer, second_offer],
    )?;
    assert_eq!(
        snapshot.definitions()[0].key().id().as_str(),
        "close-window"
    );
    assert_eq!(
        snapshot.offers()[0].reference().action(),
        snapshot.definitions()[0].key()
    );
    Ok(())
}

fn offer_for(definition: &ActionDefinition) -> Result<ActionOffer, Box<dyn std::error::Error>> {
    Ok(ActionOffer::new(
        OfferRef::new(
            definition.key().clone(),
            CatalogCodec::definition_fingerprint(definition)?,
            stamp()?,
        ),
        state()?,
        ActionAvailability::Available,
        None,
        Vec::new(),
        Vec::new(),
    )?)
}

proptest! {
    #[test]
    fn arbitrary_catalog_bytes_never_panic(bytes in prop::collection::vec(any::<u8>(), 0..65_536)) {
        let _ = CatalogCodec::decode_reply(&bytes);
    }
}
