use std::collections::BTreeMap;
use std::convert::Infallible;

use minicbor::Decoder;
use minicbor::Encoder;
use minicbor::data::Type;
use sha2::Digest;
use sha2::Sha256;
use thiserror::Error;

use super::ActionArgument;
use super::ActionArguments;
use super::ActionContractError;
use super::ActionContractFingerprint;
use super::ActionId;
use super::ActionInvocation;
use super::ActionKey;
use super::ActionSchemaVersion;
use super::ArgumentError;
use super::ArgumentScalar;
use super::ArgumentScalars;
use super::BoundedText;
use super::CatalogStamp;
use super::ChoiceId;
use super::Color;
use super::ConfirmationChallengeId;
use super::EntityId;
use super::EntityKind;
use super::EntityReference;
use super::FixedDecimal;
use super::OfferRef;
use super::ParameterId;
use super::Revision;
use super::SelectorId;
use super::StableIdError;
use super::StateStamp;
use super::Unit;
use super::UnitValue;
use super::WindowsPathInput;
use super::argument::MAX_ARGUMENTS;
use super::argument::MAX_LIST_ITEMS;
use crate::IdentifierError;
use crate::InvocationDigest;
use crate::InvocationId;
use crate::InvocationIdentityError;
use crate::InvocationNamespaceId;
use crate::InvocationSequence;
use crate::ManagerEpoch;

const MAX_INVOCATION_BYTES: usize = 16 * 1024;
const MAX_FIELDS: u64 = 32;
const MAX_SKIPPED_ITEMS: u64 = 1024;
const MAX_NESTING_DEPTH: u8 = 32;
const REQUIRED_INVOCATION_FIELDS: [u8; 4] = [0, 1, 2, 3];

#[derive(Clone, Copy, Debug, Default)]
pub struct ActionInvocationCodec;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalActionInvocation {
    bytes: Box<[u8]>,
    digest: InvocationDigest,
}

impl CanonicalActionInvocation {
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    #[must_use]
    pub const fn digest(&self) -> InvocationDigest {
        self.digest
    }

    #[must_use]
    pub fn into_bytes(self) -> Box<[u8]> {
        self.bytes
    }
}

impl ActionInvocationCodec {
    /// Encodes a canonical version 1 action invocation payload.
    ///
    /// # Errors
    ///
    /// Returns [`ActionInvocationCodecError`] when encoding fails or the final
    /// control payload exceeds 16 KiB.
    pub fn encode(invocation: &ActionInvocation) -> Result<Vec<u8>, ActionInvocationCodecError> {
        let mut encoder = Encoder::new(Vec::with_capacity(512));
        let fields = if invocation.confirmation().is_some() {
            5
        } else {
            4
        };
        encoder.map(fields)?.u8(0)?;
        encode_invocation_id(&mut encoder, invocation.invocation_id())?;
        encoder.u8(1)?;
        encode_offer(&mut encoder, invocation.offer())?;
        encoder.u8(2)?;
        encode_state(&mut encoder, invocation.expected_state())?;
        encoder.u8(3)?;
        encode_arguments(&mut encoder, invocation.arguments())?;
        if let Some(confirmation) = invocation.confirmation() {
            encoder.u8(4)?.bytes(&confirmation.into_bytes())?;
        }
        let payload = encoder.into_writer();
        if payload.len() > MAX_INVOCATION_BYTES {
            Err(ActionInvocationCodecError::PayloadTooLarge(payload.len()))
        } else {
            Ok(payload)
        }
    }

    /// Decodes a strict bounded invocation without constructing a generic CBOR value.
    ///
    /// # Errors
    ///
    /// Returns [`ActionInvocationCodecError`] for malformed, duplicate,
    /// noncanonical, oversized, or trailing input.
    pub fn decode(bytes: &[u8]) -> Result<ActionInvocation, ActionInvocationCodecError> {
        if bytes.len() > MAX_INVOCATION_BYTES {
            return Err(ActionInvocationCodecError::PayloadTooLarge(bytes.len()));
        }
        let mut decoder = Decoder::new(bytes);
        let count = definite(decoder.map()?)?;
        if count > MAX_FIELDS {
            return Err(ActionInvocationCodecError::TooManyFields(count));
        }
        let mut seen = [false; 256];
        let mut invocation_id = None;
        let mut offer = None;
        let mut expected_state = None;
        let mut arguments = None;
        let mut confirmation = None;
        for _ in 0..count {
            let key = decoder.u8()?;
            if std::mem::replace(&mut seen[usize::from(key)], true) {
                return Err(ActionInvocationCodecError::DuplicateKey(key));
            }
            match key {
                0 => invocation_id = Some(decode_invocation_id(&mut decoder)?),
                1 => offer = Some(decode_offer(&mut decoder)?),
                2 => expected_state = Some(decode_state(&mut decoder)?),
                3 => arguments = Some(decode_arguments(&mut decoder)?),
                4 => {
                    confirmation = Some(ConfirmationChallengeId::new(decode_bytes(&mut decoder)?)?);
                }
                _ => skip_bounded(&mut decoder, 0)?,
            }
        }
        for key in REQUIRED_INVOCATION_FIELDS {
            if !seen[usize::from(key)] {
                return Err(ActionInvocationCodecError::MissingKey(key));
            }
        }
        if decoder.position() != bytes.len() {
            return Err(ActionInvocationCodecError::TrailingBytes);
        }
        Ok(ActionInvocation::new(
            invocation_id.ok_or(ActionInvocationCodecError::MissingKey(0))?,
            offer.ok_or(ActionInvocationCodecError::MissingKey(1))?,
            expected_state.ok_or(ActionInvocationCodecError::MissingKey(2))?,
            arguments.ok_or(ActionInvocationCodecError::MissingKey(3))?,
            confirmation,
        ))
    }

    /// Hashes the canonical invocation bytes for durable idempotency checks.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`Self::encode`], or an identity error in the
    /// cryptographically negligible event of an all-zero SHA-256 output.
    pub fn digest(
        invocation: &ActionInvocation,
    ) -> Result<InvocationDigest, ActionInvocationCodecError> {
        Ok(Self::canonicalize(invocation)?.digest())
    }

    /// Produces the exact bytes and matching durable digest in one pass.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`Self::encode`], or an identity error in the
    /// cryptographically negligible event of an all-zero SHA-256 output.
    pub fn canonicalize(
        invocation: &ActionInvocation,
    ) -> Result<CanonicalActionInvocation, ActionInvocationCodecError> {
        let bytes = Self::encode(invocation)?.into_boxed_slice();
        let digest = InvocationDigest::new(Sha256::digest(&bytes).into())?;
        Ok(CanonicalActionInvocation { bytes, digest })
    }
}

fn encode_invocation_id(
    encoder: &mut Encoder<Vec<u8>>,
    id: InvocationId,
) -> Result<(), ActionInvocationCodecError> {
    encoder
        .array(2)?
        .bytes(&id.namespace().into_bytes())?
        .u64(id.sequence().get())?;
    Ok(())
}

fn decode_invocation_id(
    decoder: &mut Decoder<'_>,
) -> Result<InvocationId, ActionInvocationCodecError> {
    expect_array(decoder, 2)?;
    Ok(InvocationId::new(
        InvocationNamespaceId::new(decode_bytes(decoder)?)?,
        InvocationSequence::try_from(decoder.u64()?)?,
    ))
}

fn encode_offer(
    encoder: &mut Encoder<Vec<u8>>,
    offer: &OfferRef,
) -> Result<(), ActionInvocationCodecError> {
    encoder.map(3)?.u8(0)?;
    encode_action_key(encoder, offer.action())?;
    encoder
        .u8(1)?
        .bytes(&offer.contract().into_bytes())?
        .u8(2)?;
    encode_catalog(encoder, offer.catalog())
}

fn decode_offer(decoder: &mut Decoder<'_>) -> Result<OfferRef, ActionInvocationCodecError> {
    let count = bounded_map(decoder)?;
    let mut seen = [false; 256];
    let mut action = None;
    let mut contract = None;
    let mut catalog = None;
    for _ in 0..count {
        match unique_key(decoder, &mut seen)? {
            0 => action = Some(decode_action_key(decoder)?),
            1 => contract = Some(ActionContractFingerprint::new(decode_bytes(decoder)?)),
            2 => catalog = Some(decode_catalog(decoder)?),
            _ => skip_bounded(decoder, 0)?,
        }
    }
    Ok(OfferRef::new(
        required(action, 0)?,
        required(contract, 1)?,
        required(catalog, 2)?,
    ))
}

fn encode_action_key(
    encoder: &mut Encoder<Vec<u8>>,
    key: &ActionKey,
) -> Result<(), ActionInvocationCodecError> {
    encoder
        .map(2)?
        .u8(0)?
        .str(key.id().as_str())?
        .u8(1)?
        .u16(key.schema_version().get())?;
    Ok(())
}

fn decode_action_key(decoder: &mut Decoder<'_>) -> Result<ActionKey, ActionInvocationCodecError> {
    let count = bounded_map(decoder)?;
    let mut seen = [false; 256];
    let mut id = None;
    let mut schema = None;
    for _ in 0..count {
        match unique_key(decoder, &mut seen)? {
            0 => id = Some(ActionId::parse(decoder.str()?)?),
            1 => schema = Some(ActionSchemaVersion::try_from(decoder.u16()?)?),
            _ => skip_bounded(decoder, 0)?,
        }
    }
    Ok(ActionKey::new(required(id, 0)?, required(schema, 1)?))
}

fn encode_state(
    encoder: &mut Encoder<Vec<u8>>,
    state: StateStamp,
) -> Result<(), ActionInvocationCodecError> {
    encoder
        .map(2)?
        .u8(0)?
        .bytes(&state.epoch().into_bytes())?
        .u8(1)?
        .u64(state.revision().get())?;
    Ok(())
}

fn decode_state(decoder: &mut Decoder<'_>) -> Result<StateStamp, ActionInvocationCodecError> {
    let count = bounded_map(decoder)?;
    let mut seen = [false; 256];
    let mut epoch = None;
    let mut revision = None;
    for _ in 0..count {
        match unique_key(decoder, &mut seen)? {
            0 => epoch = Some(ManagerEpoch::new(decode_bytes(decoder)?)?),
            1 => revision = Some(Revision::try_from(decoder.u64()?)?),
            _ => skip_bounded(decoder, 0)?,
        }
    }
    Ok(StateStamp::new(required(epoch, 0)?, required(revision, 1)?))
}

fn encode_catalog(
    encoder: &mut Encoder<Vec<u8>>,
    catalog: CatalogStamp,
) -> Result<(), ActionInvocationCodecError> {
    encoder
        .map(4)?
        .u8(0)?
        .bytes(&catalog.epoch().into_bytes())?
        .u8(1)?
        .u64(catalog.definition_revision().get())?
        .u8(2)?
        .u64(catalog.offer_revision().get())?
        .u8(3)?
        .u64(catalog.grant_revision().get())?;
    Ok(())
}

fn decode_catalog(decoder: &mut Decoder<'_>) -> Result<CatalogStamp, ActionInvocationCodecError> {
    let count = bounded_map(decoder)?;
    let mut seen = [false; 256];
    let mut epoch = None;
    let mut definition = None;
    let mut offer = None;
    let mut grant = None;
    for _ in 0..count {
        match unique_key(decoder, &mut seen)? {
            0 => epoch = Some(ManagerEpoch::new(decode_bytes(decoder)?)?),
            1 => definition = Some(Revision::try_from(decoder.u64()?)?),
            2 => offer = Some(Revision::try_from(decoder.u64()?)?),
            3 => grant = Some(Revision::try_from(decoder.u64()?)?),
            _ => skip_bounded(decoder, 0)?,
        }
    }
    Ok(CatalogStamp::new(
        required(epoch, 0)?,
        required(definition, 1)?,
        required(offer, 2)?,
        required(grant, 3)?,
    ))
}

fn encode_arguments(
    encoder: &mut Encoder<Vec<u8>>,
    arguments: &ActionArguments,
) -> Result<(), ActionInvocationCodecError> {
    encoder.array(to_u64(arguments.values().len())?)?;
    for (id, value) in arguments.values() {
        encoder.array(2)?.str(id.as_str())?;
        encode_argument(encoder, value)?;
    }
    Ok(())
}

fn decode_arguments(
    decoder: &mut Decoder<'_>,
) -> Result<ActionArguments, ActionInvocationCodecError> {
    let count = definite(decoder.array()?)?;
    let capacity = to_usize(count)?;
    if capacity > MAX_ARGUMENTS {
        return Err(ArgumentError::TooManyArguments(capacity).into());
    }
    let mut values = BTreeMap::new();
    let mut previous: Option<ParameterId> = None;
    for _ in 0..count {
        expect_array(decoder, 2)?;
        let id = ParameterId::parse(decoder.str()?)?;
        if previous.as_ref().is_some_and(|previous| previous >= &id) {
            return Err(ActionInvocationCodecError::NonCanonicalArguments);
        }
        let value = decode_argument(decoder)?;
        previous = Some(id.clone());
        values.insert(id, value);
    }
    if values.len() != capacity {
        return Err(ActionInvocationCodecError::NonCanonicalArguments);
    }
    Ok(ActionArguments::new(values)?)
}

fn encode_argument(
    encoder: &mut Encoder<Vec<u8>>,
    value: &ActionArgument,
) -> Result<(), ActionInvocationCodecError> {
    match value {
        ActionArgument::Scalar(value) => encode_scalar(encoder, value),
        ActionArgument::Scalars(values) => {
            encoder
                .array(2)?
                .u8(0)?
                .array(to_u64(values.values().len())?)?;
            for value in values.values() {
                encode_scalar(encoder, value)?;
            }
            Ok(())
        }
    }
}

fn decode_argument(
    decoder: &mut Decoder<'_>,
) -> Result<ActionArgument, ActionInvocationCodecError> {
    let length = definite(decoder.array()?)?;
    let tag = decoder.u8()?;
    if tag != 0 {
        return Ok(ActionArgument::Scalar(decode_scalar_body(
            decoder, tag, length,
        )?));
    }
    if length != 2 {
        return Err(ActionInvocationCodecError::WrongValueLength { tag, length });
    }
    let count = definite(decoder.array()?)?;
    let capacity = to_usize(count)?;
    if capacity > MAX_LIST_ITEMS {
        return Err(ArgumentError::TooManyListItems(capacity).into());
    }
    let mut values = Vec::with_capacity(capacity);
    for _ in 0..count {
        values.push(decode_scalar(decoder)?);
    }
    Ok(ActionArgument::Scalars(ArgumentScalars::new(
        values.into_boxed_slice(),
    )?))
}

fn encode_scalar(
    encoder: &mut Encoder<Vec<u8>>,
    value: &ArgumentScalar,
) -> Result<(), ActionInvocationCodecError> {
    match value {
        ArgumentScalar::Bool(value) => {
            encoder.array(2)?.u8(1)?.bool(*value)?;
        }
        ArgumentScalar::Signed(value) => {
            encoder.array(2)?.u8(2)?.i64(*value)?;
        }
        ArgumentScalar::Unsigned(value) => {
            encoder.array(2)?.u8(3)?.u64(*value)?;
        }
        ArgumentScalar::Decimal(value) => {
            encoder
                .array(3)?
                .u8(4)?
                .i64(value.coefficient())?
                .u8(value.scale())?;
        }
        ArgumentScalar::Text(value) => {
            encoder.array(2)?.u8(5)?.str(value.as_str())?;
        }
        ArgumentScalar::Choice(value) => {
            encoder.array(2)?.u8(6)?.str(value.as_str())?;
        }
        ArgumentScalar::Color(value) => {
            let [red, green, blue, alpha] = value.channels();
            encoder
                .array(5)?
                .u8(7)?
                .u16(red)?
                .u16(green)?
                .u16(blue)?
                .u16(alpha)?;
        }
        ArgumentScalar::Unit(value) => {
            encoder
                .array(3)?
                .u8(8)?
                .u8(value.unit() as u8)?
                .i64(value.magnitude())?;
        }
        ArgumentScalar::Entity(value) => {
            encoder
                .array(3)?
                .u8(9)?
                .str(value.kind().as_str())?
                .str(value.id().as_str())?;
        }
        ArgumentScalar::Selector(value) => {
            encoder.array(2)?.u8(10)?.str(value.as_str())?;
        }
        ArgumentScalar::WindowsPath(value) => {
            let bytes = value
                .units()
                .iter()
                .flat_map(|unit| unit.to_be_bytes())
                .collect::<Vec<_>>();
            encoder.array(2)?.u8(11)?.bytes(&bytes)?;
        }
    }
    Ok(())
}

fn decode_scalar(decoder: &mut Decoder<'_>) -> Result<ArgumentScalar, ActionInvocationCodecError> {
    let length = definite(decoder.array()?)?;
    let tag = decoder.u8()?;
    if tag == 0 {
        return Err(ActionInvocationCodecError::NestedList);
    }
    decode_scalar_body(decoder, tag, length)
}

fn decode_scalar_body(
    decoder: &mut Decoder<'_>,
    tag: u8,
    length: u64,
) -> Result<ArgumentScalar, ActionInvocationCodecError> {
    let expected = match tag {
        1 | 2 | 3 | 5 | 6 | 10 | 11 => 2,
        4 | 8 | 9 => 3,
        7 => 5,
        _ => return Err(ActionInvocationCodecError::UnknownValueTag(tag)),
    };
    if length != expected {
        return Err(ActionInvocationCodecError::WrongValueLength { tag, length });
    }
    match tag {
        1 => Ok(ArgumentScalar::Bool(decoder.bool()?)),
        2 => Ok(ArgumentScalar::Signed(decoder.i64()?)),
        3 => Ok(ArgumentScalar::Unsigned(decoder.u64()?)),
        4 => Ok(ArgumentScalar::Decimal(FixedDecimal::new(
            decoder.i64()?,
            decoder.u8()?,
        )?)),
        5 => Ok(ArgumentScalar::Text(BoundedText::new(decoder.str()?)?)),
        6 => Ok(ArgumentScalar::Choice(ChoiceId::parse(decoder.str()?)?)),
        7 => Ok(ArgumentScalar::Color(Color::new(
            decoder.u16()?,
            decoder.u16()?,
            decoder.u16()?,
            decoder.u16()?,
        ))),
        8 => Ok(ArgumentScalar::Unit(UnitValue::new(
            Unit::decode(decoder.u8()?)?,
            decoder.i64()?,
        ))),
        9 => Ok(ArgumentScalar::Entity(EntityReference::new(
            EntityKind::parse(decoder.str()?)?,
            EntityId::parse(decoder.str()?)?,
        ))),
        10 => Ok(ArgumentScalar::Selector(SelectorId::parse(decoder.str()?)?)),
        11 => Ok(ArgumentScalar::WindowsPath(decode_path(decoder)?)),
        _ => Err(ActionInvocationCodecError::UnknownValueTag(tag)),
    }
}

fn decode_path(decoder: &mut Decoder<'_>) -> Result<WindowsPathInput, ActionInvocationCodecError> {
    let bytes = decoder.bytes()?;
    if bytes.len() % 2 != 0 {
        return Err(ActionInvocationCodecError::OddPathBytes(bytes.len()));
    }
    let units = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
        .collect::<Vec<_>>();
    Ok(WindowsPathInput::new(units.into_boxed_slice())?)
}

fn expect_array(
    decoder: &mut Decoder<'_>,
    expected: u64,
) -> Result<(), ActionInvocationCodecError> {
    let actual = definite(decoder.array()?)?;
    if actual == expected {
        Ok(())
    } else {
        Err(ActionInvocationCodecError::WrongArrayLength { expected, actual })
    }
}

fn bounded_map(decoder: &mut Decoder<'_>) -> Result<u64, ActionInvocationCodecError> {
    let count = definite(decoder.map()?)?;
    if count > MAX_FIELDS {
        Err(ActionInvocationCodecError::TooManyFields(count))
    } else {
        Ok(count)
    }
}

fn unique_key(
    decoder: &mut Decoder<'_>,
    seen: &mut [bool; 256],
) -> Result<u8, ActionInvocationCodecError> {
    let key = decoder.u8()?;
    if std::mem::replace(&mut seen[usize::from(key)], true) {
        Err(ActionInvocationCodecError::DuplicateKey(key))
    } else {
        Ok(key)
    }
}

fn required<T>(value: Option<T>, key: u8) -> Result<T, ActionInvocationCodecError> {
    value.ok_or(ActionInvocationCodecError::MissingKey(key))
}

fn decode_bytes<const N: usize>(
    decoder: &mut Decoder<'_>,
) -> Result<[u8; N], ActionInvocationCodecError> {
    let bytes = decoder.bytes()?;
    bytes
        .try_into()
        .map_err(|_| ActionInvocationCodecError::WrongByteLength {
            expected: N,
            actual: bytes.len(),
        })
}

fn definite(length: Option<u64>) -> Result<u64, ActionInvocationCodecError> {
    length.ok_or(ActionInvocationCodecError::IndefiniteCollection)
}

fn to_usize(value: u64) -> Result<usize, ActionInvocationCodecError> {
    usize::try_from(value).map_err(|_| ActionInvocationCodecError::CollectionTooLarge)
}

fn to_u64(value: usize) -> Result<u64, ActionInvocationCodecError> {
    u64::try_from(value).map_err(|_| ActionInvocationCodecError::CollectionTooLarge)
}

fn skip_bounded(decoder: &mut Decoder<'_>, depth: u8) -> Result<(), ActionInvocationCodecError> {
    if depth >= MAX_NESTING_DEPTH {
        return Err(ActionInvocationCodecError::NestingTooDeep);
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
        Type::F16 | Type::F32 | Type::F64 | Type::Simple => {
            decoder.skip()?;
        }
        Type::Bytes => {
            decoder.bytes()?;
        }
        Type::String => {
            decoder.str()?;
        }
        Type::Array => {
            let count = definite(decoder.array()?)?;
            ensure_skipped_bound(count)?;
            for _ in 0..count {
                skip_bounded(decoder, depth + 1)?;
            }
        }
        Type::Map => {
            let count = definite(decoder.map()?)?;
            ensure_skipped_bound(count)?;
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
            return Err(ActionInvocationCodecError::IndefiniteCollection);
        }
        Type::Break | Type::Unknown(_) => {
            return Err(ActionInvocationCodecError::UnsupportedType);
        }
    }
    Ok(())
}

fn ensure_skipped_bound(count: u64) -> Result<(), ActionInvocationCodecError> {
    if count > MAX_SKIPPED_ITEMS {
        Err(ActionInvocationCodecError::SkippedCollectionTooLarge(count))
    } else {
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum ActionInvocationCodecError {
    #[error("action invocation is {0} bytes; maximum is {MAX_INVOCATION_BYTES}")]
    PayloadTooLarge(usize),
    #[error("CBOR collections must use definite lengths")]
    IndefiniteCollection,
    #[error("invocation map has {0} fields; maximum is {MAX_FIELDS}")]
    TooManyFields(u64),
    #[error("invocation repeats numeric key {0}")]
    DuplicateKey(u8),
    #[error("invocation is missing numeric key {0}")]
    MissingKey(u8),
    #[error("invocation contains trailing bytes")]
    TrailingBytes,
    #[error("expected array length {expected}, received {actual}")]
    WrongArrayLength { expected: u64, actual: u64 },
    #[error("expected {expected} bytes, received {actual}")]
    WrongByteLength { expected: usize, actual: usize },
    #[error("argument pairs must be strictly ordered by parameter ID")]
    NonCanonicalArguments,
    #[error("unknown argument value tag {0}")]
    UnknownValueTag(u8),
    #[error("argument value tag {tag} has array length {length}")]
    WrongValueLength { tag: u8, length: u64 },
    #[error("scalar lists cannot contain another list")]
    NestedList,
    #[error("Windows path byte string has odd length {0}")]
    OddPathBytes(usize),
    #[error("collection length is outside the local address space")]
    CollectionTooLarge,
    #[error("unknown field exceeds nesting depth {MAX_NESTING_DEPTH}")]
    NestingTooDeep,
    #[error("unknown field collection has {0} items; maximum is {MAX_SKIPPED_ITEMS}")]
    SkippedCollectionTooLarge(u64),
    #[error("unknown field contains an unsupported CBOR type")]
    UnsupportedType,
    #[error(transparent)]
    StableId(#[from] StableIdError),
    #[error(transparent)]
    Argument(#[from] ArgumentError),
    #[error(transparent)]
    Contract(#[from] ActionContractError),
    #[error(transparent)]
    InvocationIdentity(#[from] InvocationIdentityError),
    #[error(transparent)]
    Identifier(#[from] IdentifierError),
    #[error(transparent)]
    Encode(#[from] minicbor::encode::Error<Infallible>),
    #[error(transparent)]
    Decode(#[from] minicbor::decode::Error),
}
