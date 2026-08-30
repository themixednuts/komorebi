use std::collections::BTreeSet;

use minicbor::Decoder;
use minicbor::Encoder;
use minicbor::data::Type;

use super::BootstrapCodec;
use super::BootstrapCodecError;
use super::FeatureId;
use super::FeatureSet;
use super::FeatureSetError;
use super::HELLO_REQUIRED_FIELDS;
use super::Hello;
use super::MAX_BOOTSTRAP_FIELDS;
use super::MAX_BOOTSTRAP_PAYLOAD_BYTES;
use super::MAX_FEATURES;
use super::MAX_NESTING_DEPTH;
use super::MAX_SKIPPED_COLLECTION_ITEMS;
use super::RoleHint;
use crate::CatalogSchemaVersion;
use crate::ProtocolMajor;
use crate::ProtocolMinor;
use crate::ProtocolVersion;
use crate::VersionRange;
use crate::VersionRanges;

impl BootstrapCodec {
    /// Encodes a canonical numeric-key, definite-length `Hello` payload.
    ///
    /// # Errors
    ///
    /// Returns `Encode` on an encoder failure.
    pub fn encode_hello(hello: &Hello) -> Result<Vec<u8>, BootstrapCodecError> {
        let field_count = if hello.requested_role_hint.is_some() {
            4
        } else {
            3
        };
        let mut encoder = Encoder::new(Vec::with_capacity(128));
        encoder.map(field_count)?.u8(0)?;
        encode_protocol_ranges(&mut encoder, hello.protocol_versions.as_slice())?;
        encoder.u8(1)?;
        encode_catalog_ranges(&mut encoder, hello.catalog_schemas.as_slice())?;
        encoder.u8(2)?;
        encode_features(&mut encoder, &hello.supported_features)?;
        if let Some(role) = hello.requested_role_hint {
            encoder.u8(3)?.u8(role as u8)?;
        }
        Ok(encoder.into_writer())
    }

    /// Decodes the bounded version 1 `Hello` shape without generic CBOR values.
    ///
    /// Unknown numeric fields are skipped only when their values remain inside
    /// the same definite-length and nesting limits as known fields.
    ///
    /// # Errors
    ///
    /// Returns a [`BootstrapCodecError`] for indefinite collections, duplicate
    /// fields, missing required fields, invalid ranges or roles, duplicate
    /// features, oversized collections, malformed CBOR, or trailing bytes.
    pub fn decode_hello(bytes: &[u8]) -> Result<Hello, BootstrapCodecError> {
        if bytes.len() > MAX_BOOTSTRAP_PAYLOAD_BYTES {
            return Err(BootstrapCodecError::BootstrapPayloadTooLarge(bytes.len()));
        }
        let mut decoder = Decoder::new(bytes);
        let field_count = definite_len(decoder.map()?)?;
        if field_count > MAX_BOOTSTRAP_FIELDS {
            return Err(BootstrapCodecError::WrongMapLength(field_count));
        }
        let mut seen = [false; 256];
        let mut protocol_versions = None;
        let mut catalog_schemas = None;
        let mut supported_features = None;
        let mut requested_role_hint = None;

        for _ in 0..field_count {
            let key = decoder.u8()?;
            let slot = &mut seen[usize::from(key)];
            if std::mem::replace(slot, true) {
                return Err(BootstrapCodecError::DuplicateKey(key));
            }
            match key {
                0 => protocol_versions = Some(decode_protocol_ranges(&mut decoder)?),
                1 => catalog_schemas = Some(decode_catalog_ranges(&mut decoder)?),
                2 => supported_features = Some(decode_features(&mut decoder)?),
                3 => requested_role_hint = Some(RoleHint::decode(decoder.u8()?)?),
                _ => skip_bounded(&mut decoder, 0)?,
            }
        }

        for key in HELLO_REQUIRED_FIELDS {
            if !seen[usize::from(key)] {
                return Err(BootstrapCodecError::MissingKey(key));
            }
        }
        if decoder.position() != bytes.len() {
            return Err(BootstrapCodecError::TrailingBytes);
        }
        Ok(Hello::new(
            protocol_versions.ok_or(BootstrapCodecError::MissingKey(0))?,
            catalog_schemas.ok_or(BootstrapCodecError::MissingKey(1))?,
            supported_features.ok_or(BootstrapCodecError::MissingKey(2))?,
            requested_role_hint,
        ))
    }
}

pub(super) fn encode_protocol_ranges(
    encoder: &mut Encoder<Vec<u8>>,
    ranges: &[VersionRange<ProtocolVersion>],
) -> Result<(), BootstrapCodecError> {
    encoder.array(usize_to_u64(ranges.len())?)?;
    for range in ranges {
        encoder.array(2)?;
        encode_protocol_version(encoder, range.first())?;
        encode_protocol_version(encoder, range.last())?;
    }
    Ok(())
}

pub(super) fn encode_protocol_version(
    encoder: &mut Encoder<Vec<u8>>,
    version: ProtocolVersion,
) -> Result<(), BootstrapCodecError> {
    encoder
        .array(2)?
        .u16(version.major().get())?
        .u16(version.minor().get())?;
    Ok(())
}

pub(super) fn encode_catalog_ranges(
    encoder: &mut Encoder<Vec<u8>>,
    ranges: &[VersionRange<CatalogSchemaVersion>],
) -> Result<(), BootstrapCodecError> {
    encoder.array(usize_to_u64(ranges.len())?)?;
    for range in ranges {
        encoder
            .array(2)?
            .u16(range.first().get())?
            .u16(range.last().get())?;
    }
    Ok(())
}

pub(super) fn decode_protocol_ranges(
    decoder: &mut Decoder<'_>,
) -> Result<VersionRanges<ProtocolVersion>, BootstrapCodecError> {
    decode_ranges(decoder, decode_protocol_version)
}

pub(super) fn decode_catalog_ranges(
    decoder: &mut Decoder<'_>,
) -> Result<VersionRanges<CatalogSchemaVersion>, BootstrapCodecError> {
    decode_ranges(decoder, |decoder| {
        Ok(CatalogSchemaVersion::try_from(decoder.u16()?)?)
    })
}

fn decode_ranges<V>(
    decoder: &mut Decoder<'_>,
    version: impl Fn(&mut Decoder<'_>) -> Result<V, BootstrapCodecError>,
) -> Result<VersionRanges<V>, BootstrapCodecError>
where
    V: Copy + Ord,
{
    let count = definite_len(decoder.array()?)?;
    let capacity = usize::try_from(count).map_err(|_| BootstrapCodecError::CollectionTooLarge)?;
    if capacity == 0 || capacity > 32 {
        return Err(BootstrapCodecError::InvalidRangeCount(capacity));
    }
    let mut ranges = Vec::with_capacity(capacity);
    for _ in 0..count {
        let range_len = definite_len(decoder.array()?)?;
        if range_len != 2 {
            return Err(BootstrapCodecError::WrongRangeLength(range_len));
        }
        ranges.push(VersionRange::new(version(decoder)?, version(decoder)?)?);
    }
    Ok(VersionRanges::new(ranges)?)
}

pub(super) fn decode_protocol_version(
    decoder: &mut Decoder<'_>,
) -> Result<ProtocolVersion, BootstrapCodecError> {
    let length = definite_len(decoder.array()?)?;
    if length != 2 {
        return Err(BootstrapCodecError::WrongProtocolVersionLength(length));
    }
    Ok(ProtocolVersion::new(
        ProtocolMajor::try_from(decoder.u16()?)?,
        ProtocolMinor::new(decoder.u16()?),
    ))
}

pub(super) fn encode_features(
    encoder: &mut Encoder<Vec<u8>>,
    features: &FeatureSet,
) -> Result<(), BootstrapCodecError> {
    encoder.array(usize_to_u64(features.as_set().len())?)?;
    for feature in features.as_set() {
        encoder.u32(feature.get())?;
    }
    Ok(())
}

pub(super) fn decode_features(
    decoder: &mut Decoder<'_>,
) -> Result<FeatureSet, BootstrapCodecError> {
    let count = definite_len(decoder.array()?)?;
    let capacity = usize::try_from(count).map_err(|_| BootstrapCodecError::CollectionTooLarge)?;
    if capacity > MAX_FEATURES {
        return Err(FeatureSetError::TooMany(capacity).into());
    }
    let mut features = BTreeSet::new();
    for _ in 0..count {
        let feature = FeatureId::try_from(decoder.u32()?)?;
        if !features.insert(feature) {
            return Err(BootstrapCodecError::DuplicateFeature(feature));
        }
    }
    Ok(FeatureSet::new(features)?)
}

pub(super) fn definite_len(length: Option<u64>) -> Result<u64, BootstrapCodecError> {
    length.ok_or(BootstrapCodecError::IndefiniteCollection)
}

pub(super) fn usize_to_u64(value: usize) -> Result<u64, BootstrapCodecError> {
    u64::try_from(value).map_err(|_| BootstrapCodecError::CollectionTooLarge)
}

pub(super) fn skip_bounded(
    decoder: &mut Decoder<'_>,
    depth: u8,
) -> Result<(), BootstrapCodecError> {
    if depth >= MAX_NESTING_DEPTH {
        return Err(BootstrapCodecError::NestingTooDeep);
    }
    match decoder.datatype()? {
        Type::Bool => {
            decoder.bool()?;
        }
        Type::Null => decoder.null()?,
        Type::Undefined => decoder.undefined()?,
        Type::U8 | Type::U16 | Type::U32 | Type::U64 => {
            decoder.u64()?;
        }
        Type::I8 | Type::I16 | Type::I32 | Type::I64 | Type::Int => {
            decoder.int()?;
        }
        Type::F16 => {
            decoder.skip()?;
        }
        Type::F32 => {
            decoder.f32()?;
        }
        Type::F64 => {
            decoder.f64()?;
        }
        Type::Simple => {
            decoder.simple()?;
        }
        Type::Bytes => {
            decoder.bytes()?;
        }
        Type::String => {
            decoder.str()?;
        }
        Type::Array => {
            let count = definite_len(decoder.array()?)?;
            ensure_skipped_collection_bound(count)?;
            for _ in 0..count {
                skip_bounded(decoder, depth + 1)?;
            }
        }
        Type::Map => {
            let count = definite_len(decoder.map()?)?;
            ensure_skipped_collection_bound(count)?;
            for _ in 0..count {
                skip_bounded(decoder, depth + 1)?;
                skip_bounded(decoder, depth + 1)?;
            }
        }
        Type::Tag => {
            decoder.tag()?;
            skip_bounded(decoder, depth + 1)?;
        }
        Type::BytesIndef | Type::StringIndef | Type::ArrayIndef | Type::MapIndef => {
            return Err(BootstrapCodecError::IndefiniteCollection);
        }
        Type::Break | Type::Unknown(_) => {
            return Err(BootstrapCodecError::UnsupportedType);
        }
    }
    Ok(())
}

fn ensure_skipped_collection_bound(count: u64) -> Result<(), BootstrapCodecError> {
    if count > MAX_SKIPPED_COLLECTION_ITEMS {
        Err(BootstrapCodecError::SkippedCollectionTooLarge(count))
    } else {
        Ok(())
    }
}
