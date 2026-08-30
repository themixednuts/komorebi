use minicbor::Decoder;
use minicbor::Encoder;

use super::ActionContractFingerprint;
use super::ActionDefinition;
use super::ActionOffer;
use super::CatalogQuery;
use super::CatalogReply;
use super::CatalogSnapshot;
use super::CommandCodecError;
use super::catalog::MAX_CATALOG_ACTIONS;
use super::catalog_definition_codec::decode_definition;
use super::catalog_definition_codec::encode_definition;
use super::catalog_definition_codec::fingerprint;
use super::catalog_offer_codec::decode_action_offer;
use super::catalog_offer_codec::encode_action_offer;
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
use super::codec::to_u64;
use super::codec::to_usize;
use super::codec::unique_key;

const MAX_CATALOG_PAYLOAD_BYTES: usize = 8 * 1024 * 1024;
const NOT_MODIFIED_TAG: u8 = 1;
const SNAPSHOT_TAG: u8 = 2;

#[derive(Clone, Copy, Debug, Default)]
pub struct CatalogCodec;

impl CatalogCodec {
    /// Encodes an optional client cache stamp.
    ///
    /// # Errors
    ///
    /// Returns [`CommandCodecError`] when encoding fails or exceeds the
    /// negotiated command-control payload bound.
    pub fn encode_query(query: CatalogQuery) -> Result<Vec<u8>, CommandCodecError> {
        let mut encoder = Encoder::new(Vec::with_capacity(64));
        match query.known() {
            Some(known) => {
                encoder.map(1)?.u8(0)?;
                encode_catalog(&mut encoder, known)?;
            }
            None => {
                encoder.map(0)?;
            }
        }
        let bytes = encoder.into_writer();
        ensure_command_bound(&bytes)?;
        Ok(bytes)
    }

    /// Decodes a strict, bounded catalog query.
    ///
    /// # Errors
    ///
    /// Returns [`CommandCodecError`] for malformed, duplicate, indefinite,
    /// oversized, or trailing input.
    pub fn decode_query(bytes: &[u8]) -> Result<CatalogQuery, CommandCodecError> {
        ensure_command_bound(bytes)?;
        let mut decoder = Decoder::new(bytes);
        let count = bounded_map(&mut decoder)?;
        let mut seen = [false; 256];
        let mut known = None;
        for _ in 0..count {
            match unique_key(&mut decoder, &mut seen)? {
                0 => known = Some(decode_catalog(&mut decoder)?),
                _ => skip_bounded(&mut decoder, 0)?,
            }
        }
        require_eof(&decoder, bytes)?;
        Ok(CatalogQuery::new(known))
    }

    /// Encodes an immutable catalog result as one logical payload. The
    /// transport may split this payload into bounded snapshot frames.
    ///
    /// # Errors
    ///
    /// Returns [`CommandCodecError`] for encoding failure, an inconsistent
    /// action fingerprint, or a payload above the 8 MiB reassembly bound.
    pub fn encode_reply(reply: &CatalogReply) -> Result<Vec<u8>, CommandCodecError> {
        let mut encoder = Encoder::new(Vec::with_capacity(16 * 1024));
        encoder.map(2)?.u8(0)?;
        match reply {
            CatalogReply::NotModified(stamp) => {
                encoder.u8(NOT_MODIFIED_TAG)?.u8(1)?;
                encode_catalog(&mut encoder, *stamp)?;
            }
            CatalogReply::Snapshot(snapshot) => {
                validate_fingerprints(snapshot)?;
                encoder.u8(SNAPSHOT_TAG)?.u8(1)?;
                encode_snapshot(&mut encoder, snapshot)?;
            }
        }
        bounded_catalog(encoder.into_writer())
    }

    /// Decodes one reassembled immutable catalog result.
    ///
    /// # Errors
    ///
    /// Returns [`CommandCodecError`] for malformed, duplicate, unknown,
    /// inconsistent, oversized, indefinite, or trailing input.
    pub fn decode_reply(bytes: &[u8]) -> Result<CatalogReply, CommandCodecError> {
        ensure_catalog_bound(bytes)?;
        let mut decoder = Decoder::new(bytes);
        let count = bounded_map(&mut decoder)?;
        let mut seen = [false; 256];
        let mut tag = None;
        let mut not_modified = None;
        let mut snapshot = None;
        for _ in 0..count {
            match unique_key(&mut decoder, &mut seen)? {
                0 => tag = Some(decoder.u8()?),
                1 => match required(tag, 0)? {
                    NOT_MODIFIED_TAG => not_modified = Some(decode_catalog(&mut decoder)?),
                    SNAPSHOT_TAG => snapshot = Some(decode_snapshot(&mut decoder)?),
                    value => return Err(CommandCodecError::UnknownCatalogReplyTag(value)),
                },
                _ => skip_bounded(&mut decoder, 0)?,
            }
        }
        require_eof(&decoder, bytes)?;
        match required(tag, 0)? {
            NOT_MODIFIED_TAG => Ok(CatalogReply::NotModified(required(not_modified, 1)?)),
            SNAPSHOT_TAG => {
                let snapshot = required(snapshot, 1)?;
                validate_fingerprints(&snapshot)?;
                Ok(CatalogReply::Snapshot(snapshot))
            }
            value => Err(CommandCodecError::UnknownCatalogReplyTag(value)),
        }
    }

    /// Hashes the canonical stable definition fields used by [`OfferRef`](super::OfferRef).
    ///
    /// # Errors
    ///
    /// Returns [`CommandCodecError`] when canonical encoding fails.
    pub fn definition_fingerprint(
        definition: &ActionDefinition,
    ) -> Result<ActionContractFingerprint, CommandCodecError> {
        fingerprint(definition)
    }
}

fn encode_snapshot(
    encoder: &mut Encoder<Vec<u8>>,
    snapshot: &CatalogSnapshot,
) -> Result<(), CommandCodecError> {
    encoder.map(4)?.u8(0)?;
    encode_catalog(encoder, snapshot.stamp())?;
    encoder.u8(1)?;
    encode_state(encoder, snapshot.state())?;
    encoder
        .u8(2)?
        .array(to_u64(snapshot.definitions().len())?)?;
    for definition in snapshot.definitions() {
        encode_definition(encoder, definition)?;
    }
    encoder.u8(3)?.array(to_u64(snapshot.offers().len())?)?;
    for offer in snapshot.offers() {
        encode_action_offer(encoder, offer)?;
    }
    Ok(())
}

fn decode_snapshot(decoder: &mut Decoder<'_>) -> Result<CatalogSnapshot, CommandCodecError> {
    let count = bounded_map(decoder)?;
    let mut seen = [false; 256];
    let mut stamp = None;
    let mut state = None;
    let mut definitions = None;
    let mut offers = None;
    for _ in 0..count {
        match unique_key(decoder, &mut seen)? {
            0 => stamp = Some(decode_catalog(decoder)?),
            1 => state = Some(decode_state(decoder)?),
            2 => definitions = Some(decode_definitions(decoder)?),
            3 => offers = Some(decode_offers(decoder)?),
            _ => skip_bounded(decoder, 0)?,
        }
    }
    Ok(CatalogSnapshot::new(
        required(stamp, 0)?,
        required(state, 1)?,
        required(definitions, 2)?,
        required(offers, 3)?,
    )?)
}

fn decode_definitions(
    decoder: &mut Decoder<'_>,
) -> Result<Vec<ActionDefinition>, CommandCodecError> {
    let count = catalog_count(decoder)?;
    let mut definitions = Vec::with_capacity(count);
    for _ in 0..count {
        definitions.push(decode_definition(decoder)?);
    }
    Ok(definitions)
}

fn decode_offers(decoder: &mut Decoder<'_>) -> Result<Vec<ActionOffer>, CommandCodecError> {
    let count = catalog_count(decoder)?;
    let mut offers = Vec::with_capacity(count);
    for _ in 0..count {
        offers.push(decode_action_offer(decoder)?);
    }
    Ok(offers)
}

fn catalog_count(decoder: &mut Decoder<'_>) -> Result<usize, CommandCodecError> {
    let count = to_usize(definite(decoder.array()?)?)?;
    if count > MAX_CATALOG_ACTIONS {
        Err(CommandCodecError::CatalogCollectionTooLarge {
            actual: count,
            maximum: MAX_CATALOG_ACTIONS,
        })
    } else {
        Ok(count)
    }
}

fn validate_fingerprints(snapshot: &CatalogSnapshot) -> Result<(), CommandCodecError> {
    for (definition, offer) in snapshot.definitions().iter().zip(snapshot.offers()) {
        if fingerprint(definition)? != offer.reference().contract() {
            return Err(CommandCodecError::DefinitionFingerprintMismatch);
        }
    }
    Ok(())
}

fn bounded_catalog(bytes: Vec<u8>) -> Result<Vec<u8>, CommandCodecError> {
    ensure_catalog_bound(&bytes)?;
    Ok(bytes)
}

fn ensure_catalog_bound(bytes: &[u8]) -> Result<(), CommandCodecError> {
    if bytes.len() > MAX_CATALOG_PAYLOAD_BYTES {
        Err(CommandCodecError::CatalogPayloadTooLarge(bytes.len()))
    } else {
        Ok(())
    }
}
