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

pub(super) const MAX_COMMAND_PAYLOAD_BYTES: usize = 16 * 1024;
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
    /// Returns [`CommandCodecError`] when encoding fails or the final
    /// control payload exceeds 16 KiB.
    pub fn encode(invocation: &ActionInvocation) -> Result<Vec<u8>, CommandCodecError> {
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
        if payload.len() > MAX_COMMAND_PAYLOAD_BYTES {
            Err(CommandCodecError::PayloadTooLarge(payload.len()))
        } else {
            Ok(payload)
        }
    }

    /// Decodes a strict bounded invocation without constructing a generic CBOR value.
    ///
    /// # Errors
    ///
    /// Returns [`CommandCodecError`] for malformed, duplicate,
    /// noncanonical, oversized, or trailing input.
    pub fn decode(bytes: &[u8]) -> Result<ActionInvocation, CommandCodecError> {
        if bytes.len() > MAX_COMMAND_PAYLOAD_BYTES {
            return Err(CommandCodecError::PayloadTooLarge(bytes.len()));
        }
        let mut decoder = Decoder::new(bytes);
        let count = definite(decoder.map()?)?;
        if count > MAX_FIELDS {
            return Err(CommandCodecError::TooManyFields(count));
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
                return Err(CommandCodecError::DuplicateKey(key));
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
                return Err(CommandCodecError::MissingKey(key));
            }
        }
        if decoder.position() != bytes.len() {
            return Err(CommandCodecError::TrailingBytes);
        }
        Ok(ActionInvocation::new(
            invocation_id.ok_or(CommandCodecError::MissingKey(0))?,
            offer.ok_or(CommandCodecError::MissingKey(1))?,
            expected_state.ok_or(CommandCodecError::MissingKey(2))?,
            arguments.ok_or(CommandCodecError::MissingKey(3))?,
            confirmation,
        ))
    }

    /// Hashes the canonical invocation bytes for durable idempotency checks.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`Self::encode`], or an identity error in the
    /// cryptographically negligible event of an all-zero SHA-256 output.
    pub fn digest(invocation: &ActionInvocation) -> Result<InvocationDigest, CommandCodecError> {
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
    ) -> Result<CanonicalActionInvocation, CommandCodecError> {
        let bytes = Self::encode(invocation)?.into_boxed_slice();
        let digest = InvocationDigest::new(Sha256::digest(&bytes).into())?;
        Ok(CanonicalActionInvocation { bytes, digest })
    }
}

pub(super) fn encode_invocation_id(
    encoder: &mut Encoder<Vec<u8>>,
    id: InvocationId,
) -> Result<(), CommandCodecError> {
    encoder
        .array(2)?
        .bytes(&id.namespace().into_bytes())?
        .u64(id.sequence().get())?;
    Ok(())
}

pub(super) fn decode_invocation_id(
    decoder: &mut Decoder<'_>,
) -> Result<InvocationId, CommandCodecError> {
    expect_array(decoder, 2)?;
    Ok(InvocationId::new(
        InvocationNamespaceId::new(decode_bytes(decoder)?)?,
        InvocationSequence::try_from(decoder.u64()?)?,
    ))
}

fn encode_offer(encoder: &mut Encoder<Vec<u8>>, offer: &OfferRef) -> Result<(), CommandCodecError> {
    encoder.map(3)?.u8(0)?;
    encode_action_key(encoder, offer.action())?;
    encoder
        .u8(1)?
        .bytes(&offer.contract().into_bytes())?
        .u8(2)?;
    encode_catalog(encoder, offer.catalog())
}

fn decode_offer(decoder: &mut Decoder<'_>) -> Result<OfferRef, CommandCodecError> {
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
) -> Result<(), CommandCodecError> {
    encoder
        .map(2)?
        .u8(0)?
        .str(key.id().as_str())?
        .u8(1)?
        .u16(key.schema_version().get())?;
    Ok(())
}

fn decode_action_key(decoder: &mut Decoder<'_>) -> Result<ActionKey, CommandCodecError> {
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

pub(super) fn encode_state(
    encoder: &mut Encoder<Vec<u8>>,
    state: StateStamp,
) -> Result<(), CommandCodecError> {
    encoder
        .map(2)?
        .u8(0)?
        .bytes(&state.epoch().into_bytes())?
        .u8(1)?
        .u64(state.revision().get())?;
    Ok(())
}

pub(super) fn decode_state(decoder: &mut Decoder<'_>) -> Result<StateStamp, CommandCodecError> {
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
) -> Result<(), CommandCodecError> {
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

fn decode_catalog(decoder: &mut Decoder<'_>) -> Result<CatalogStamp, CommandCodecError> {
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
) -> Result<(), CommandCodecError> {
    encoder.array(to_u64(arguments.values().len())?)?;
    for (id, value) in arguments.values() {
        encoder.array(2)?.str(id.as_str())?;
        encode_argument(encoder, value)?;
    }
    Ok(())
}

fn decode_arguments(decoder: &mut Decoder<'_>) -> Result<ActionArguments, CommandCodecError> {
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
            return Err(CommandCodecError::NonCanonicalArguments);
        }
        let value = decode_argument(decoder)?;
        previous = Some(id.clone());
        values.insert(id, value);
    }
    if values.len() != capacity {
        return Err(CommandCodecError::NonCanonicalArguments);
    }
    Ok(ActionArguments::new(values)?)
}

fn encode_argument(
    encoder: &mut Encoder<Vec<u8>>,
    value: &ActionArgument,
) -> Result<(), CommandCodecError> {
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

fn decode_argument(decoder: &mut Decoder<'_>) -> Result<ActionArgument, CommandCodecError> {
    let length = definite(decoder.array()?)?;
    let tag = decoder.u8()?;
    if tag != 0 {
        return Ok(ActionArgument::Scalar(decode_scalar_body(
            decoder, tag, length,
        )?));
    }
    if length != 2 {
        return Err(CommandCodecError::WrongValueLength { tag, length });
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
) -> Result<(), CommandCodecError> {
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

fn decode_scalar(decoder: &mut Decoder<'_>) -> Result<ArgumentScalar, CommandCodecError> {
    let length = definite(decoder.array()?)?;
    let tag = decoder.u8()?;
    if tag == 0 {
        return Err(CommandCodecError::NestedList);
    }
    decode_scalar_body(decoder, tag, length)
}

fn decode_scalar_body(
    decoder: &mut Decoder<'_>,
    tag: u8,
    length: u64,
) -> Result<ArgumentScalar, CommandCodecError> {
    let expected = match tag {
        1 | 2 | 3 | 5 | 6 | 10 | 11 => 2,
        4 | 8 | 9 => 3,
        7 => 5,
        _ => return Err(CommandCodecError::UnknownValueTag(tag)),
    };
    if length != expected {
        return Err(CommandCodecError::WrongValueLength { tag, length });
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
        _ => Err(CommandCodecError::UnknownValueTag(tag)),
    }
}

fn decode_path(decoder: &mut Decoder<'_>) -> Result<WindowsPathInput, CommandCodecError> {
    let bytes = decoder.bytes()?;
    if bytes.len() % 2 != 0 {
        return Err(CommandCodecError::OddPathBytes(bytes.len()));
    }
    let units = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
        .collect::<Vec<_>>();
    Ok(WindowsPathInput::new(units.into_boxed_slice())?)
}

fn expect_array(decoder: &mut Decoder<'_>, expected: u64) -> Result<(), CommandCodecError> {
    let actual = definite(decoder.array()?)?;
    if actual == expected {
        Ok(())
    } else {
        Err(CommandCodecError::WrongArrayLength { expected, actual })
    }
}

pub(super) fn bounded_map(decoder: &mut Decoder<'_>) -> Result<u64, CommandCodecError> {
    let count = definite(decoder.map()?)?;
    if count > MAX_FIELDS {
        Err(CommandCodecError::TooManyFields(count))
    } else {
        Ok(count)
    }
}

pub(super) fn unique_key(
    decoder: &mut Decoder<'_>,
    seen: &mut [bool; 256],
) -> Result<u8, CommandCodecError> {
    let key = decoder.u8()?;
    if std::mem::replace(&mut seen[usize::from(key)], true) {
        Err(CommandCodecError::DuplicateKey(key))
    } else {
        Ok(key)
    }
}

pub(super) fn required<T>(value: Option<T>, key: u8) -> Result<T, CommandCodecError> {
    value.ok_or(CommandCodecError::MissingKey(key))
}

pub(super) fn bounded_encoded(bytes: Vec<u8>) -> Result<Vec<u8>, CommandCodecError> {
    ensure_command_bound(&bytes)?;
    Ok(bytes)
}

pub(super) fn ensure_command_bound(bytes: &[u8]) -> Result<(), CommandCodecError> {
    if bytes.len() > MAX_COMMAND_PAYLOAD_BYTES {
        Err(CommandCodecError::PayloadTooLarge(bytes.len()))
    } else {
        Ok(())
    }
}

pub(super) fn require_eof(decoder: &Decoder<'_>, bytes: &[u8]) -> Result<(), CommandCodecError> {
    if decoder.position() == bytes.len() {
        Ok(())
    } else {
        Err(CommandCodecError::TrailingBytes)
    }
}

pub(super) fn decode_bytes<const N: usize>(
    decoder: &mut Decoder<'_>,
) -> Result<[u8; N], CommandCodecError> {
    let bytes = decoder.bytes()?;
    bytes
        .try_into()
        .map_err(|_| CommandCodecError::WrongByteLength {
            expected: N,
            actual: bytes.len(),
        })
}

pub(super) fn definite(length: Option<u64>) -> Result<u64, CommandCodecError> {
    length.ok_or(CommandCodecError::IndefiniteCollection)
}

fn to_usize(value: u64) -> Result<usize, CommandCodecError> {
    usize::try_from(value).map_err(|_| CommandCodecError::CollectionTooLarge)
}

fn to_u64(value: usize) -> Result<u64, CommandCodecError> {
    u64::try_from(value).map_err(|_| CommandCodecError::CollectionTooLarge)
}

pub(super) fn skip_bounded(decoder: &mut Decoder<'_>, depth: u8) -> Result<(), CommandCodecError> {
    if depth >= MAX_NESTING_DEPTH {
        return Err(CommandCodecError::NestingTooDeep);
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
            return Err(CommandCodecError::IndefiniteCollection);
        }
        Type::Break | Type::Unknown(_) => {
            return Err(CommandCodecError::UnsupportedType);
        }
    }
    Ok(())
}

fn ensure_skipped_bound(count: u64) -> Result<(), CommandCodecError> {
    if count > MAX_SKIPPED_ITEMS {
        Err(CommandCodecError::SkippedCollectionTooLarge(count))
    } else {
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum CommandCodecError {
    #[error("command payload is {0} bytes; maximum is {MAX_COMMAND_PAYLOAD_BYTES}")]
    PayloadTooLarge(usize),
    #[error("CBOR collections must use definite lengths")]
    IndefiniteCollection,
    #[error("command map has {0} fields; maximum is {MAX_FIELDS}")]
    TooManyFields(u64),
    #[error("command repeats numeric key {0}")]
    DuplicateKey(u8),
    #[error("command is missing numeric key {0}")]
    MissingKey(u8),
    #[error("command contains trailing bytes")]
    TrailingBytes,
    #[error("unknown invocation lease reply tag {0}")]
    UnknownLeaseReplyTag(u8),
    #[error("unknown invocation lease rejection code {0}")]
    UnknownLeaseRejection(u8),
    #[error("invocation lease count must be nonzero")]
    ZeroLeaseCount,
    #[error("invocation lease reply field 1 has the wrong CBOR type")]
    WrongLeaseReplyFieldType,
    #[error("invocation lease reply tag {tag} cannot contain numeric key {key}")]
    UnexpectedLeaseReplyField { tag: u8, key: u8 },
    #[error("unknown invocation status reply tag {0}")]
    UnknownStatusReplyTag(u8),
    #[error("unknown invocation cancellation reply tag {0}")]
    UnknownCancelReplyTag(u8),
    #[error("unknown invocation availability code {0}")]
    UnknownInvocationUnavailable(u8),
    #[error("invocation control reply tag {0} has the wrong field 1 CBOR type")]
    WrongInvocationControlReplyFieldType(u8),
    #[error("unknown invocation progress tag {0}")]
    UnknownInvocationProgressTag(u8),
    #[error("invocation progress tag {tag} has array length {length}")]
    WrongInvocationProgressLength { tag: u8, length: u64 },
    #[error("unknown invocation terminal tag {0}")]
    UnknownInvocationTerminalTag(u8),
    #[error("invocation terminal tag {tag} has array length {length}")]
    WrongInvocationTerminalLength { tag: u8, length: u64 },
    #[error("unknown settled invocation kind {0}")]
    UnknownSettledInvocationKind(u8),
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
