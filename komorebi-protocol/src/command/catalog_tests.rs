use std::num::NonZeroU16;

use proptest::prelude::*;

use super::*;
use crate::AssemblyDeadlineMs;
use crate::ChunkPayloadLimit;
use crate::ControlPayloadLimit;
use crate::FramePayloadLimit;
use crate::ManagerEpoch;
use crate::NestingLimit;
use crate::ReassemblyLimit;
use crate::SessionLimits;

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
            ArgumentCardinality::RequiredScalar,
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
    let decoded = CatalogCodec::decode_reply(&encoded)?;
    assert_eq!(decoded, reply);
    let CatalogReply::Snapshot(snapshot) = decoded else {
        return Err("snapshot reply changed variant".into());
    };
    assert_eq!(
        snapshot.definitions()[0].parameters()[0].cardinality(),
        ArgumentCardinality::RequiredScalar
    );
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

fn narrow_transfer_limits() -> Result<SessionLimits, Box<dyn std::error::Error>> {
    Ok(SessionLimits::new(
        FramePayloadLimit::new(128)?,
        ControlPayloadLimit::new(64)?,
        ChunkPayloadLimit::new(48)?,
        ReassemblyLimit::new(1024 * 1024)?,
        NestingLimit::new(32)?,
        AssemblyDeadlineMs::new(2_000)?,
    )?)
}

#[test]
fn chunked_catalog_round_trip_uses_negotiated_frame_bounds()
-> Result<(), Box<dyn std::error::Error>> {
    let limits = narrow_transfer_limits()?;
    let reply = CatalogReply::Snapshot(snapshot()?);
    let chunks = encoded_chunks(&reply, limits)?;
    assert!(chunks.len() > 1);
    assert!(
        chunks
            .iter()
            .all(|chunk| chunk.len() <= limits.chunk_payload().get() as usize)
    );

    let mut reassembler = CatalogReassembler::new(limits);
    let mut completed = None;
    for chunk in chunks {
        completed = reassembler.push(&chunk)?;
    }
    assert_eq!(completed, Some(reply));
    assert!(!reassembler.is_pending());
    Ok(())
}

#[test]
fn catalog_reassembly_rejects_gaps_and_replays() -> Result<(), Box<dyn std::error::Error>> {
    let limits = narrow_transfer_limits()?;
    let reply = CatalogReply::NotModified(stamp()?);
    let chunks = encoded_chunks(&reply, limits)?;
    assert!(chunks.len() > 1);

    let mut reassembler = CatalogReassembler::new(limits);
    assert!(matches!(
        reassembler.push(&chunks[1]),
        Err(CatalogTransferError::NonContiguous {
            expected: 0,
            actual: _
        })
    ));
    assert_eq!(reassembler.push(&chunks[0])?, None);
    assert!(matches!(
        reassembler.push(&chunks[0]),
        Err(CatalogTransferError::NonContiguous {
            expected: _,
            actual: 0
        })
    ));
    Ok(())
}

#[test]
fn catalog_reassembly_verifies_the_completed_digest() -> Result<(), Box<dyn std::error::Error>> {
    let limits = narrow_transfer_limits()?;
    let reply = CatalogReply::Snapshot(snapshot()?);
    let mut chunks = encoded_chunks(&reply, limits)?
        .into_iter()
        .map(Vec::from)
        .collect::<Vec<_>>();
    let final_byte = chunks
        .last_mut()
        .and_then(|chunk| chunk.last_mut())
        .ok_or("catalog test requires a final payload byte")?;
    *final_byte ^= 1;

    let mut reassembler = CatalogReassembler::new(limits);
    let last = chunks.len().saturating_sub(1);
    for chunk in &chunks[..last] {
        assert_eq!(reassembler.push(chunk)?, None);
    }
    assert!(matches!(
        reassembler.push(&chunks[last]),
        Err(CatalogTransferError::DigestMismatch)
    ));
    Ok(())
}

fn encoded_chunks(
    reply: &CatalogReply,
    limits: SessionLimits,
) -> Result<Vec<Box<[u8]>>, CatalogTransferError> {
    let mut transfer = CatalogChunks::new(reply, limits)?;
    let mut chunks = Vec::new();
    while let Some(chunk) = transfer.next_chunk()? {
        chunks.push(chunk.encode());
    }
    Ok(chunks)
}

proptest! {
    #[test]
    fn arbitrary_catalog_bytes_never_panic(bytes in prop::collection::vec(any::<u8>(), 0..65_536)) {
        let _ = CatalogCodec::decode_reply(&bytes);
    }

    #[test]
    fn arbitrary_catalog_chunks_never_panic(bytes in prop::collection::vec(any::<u8>(), 0..65_536)) {
        let mut reassembler = CatalogReassembler::new(SessionLimits::V1);
        let _ = reassembler.push(&bytes);
    }
}
