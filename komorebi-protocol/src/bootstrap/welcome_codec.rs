use std::collections::BTreeSet;

use minicbor::Decoder;
use minicbor::Encoder;

use super::BootstrapCodec;
use super::BootstrapCodecError;
use super::MAX_BOOTSTRAP_FIELDS;
use super::MAX_BOOTSTRAP_PAYLOAD_BYTES;
use super::codec::decode_features;
use super::codec::decode_protocol_version;
use super::codec::definite_len;
use super::codec::encode_features;
use super::codec::encode_protocol_version;
use super::codec::skip_bounded;
use super::codec::usize_to_u64;
use crate::AssemblyDeadlineMs;
use crate::AuthorityCapabilityId;
use crate::AuthoritySummary;
use crate::CatalogSchemaVersion;
use crate::ChunkPayloadLimit;
use crate::ConnectionId;
use crate::ControlPayloadLimit;
use crate::FramePayloadLimit;
use crate::ManagerEpoch;
use crate::NegotiatedProtocol;
use crate::NestingLimit;
use crate::ReassemblyLimit;
use crate::SessionLimits;
use crate::Welcome;

const WELCOME_REQUIRED_FIELDS: [u8; 7] = [0, 1, 2, 3, 4, 5, 6];
const LIMIT_REQUIRED_FIELDS: [u8; 6] = [0, 1, 2, 3, 4, 5];

impl BootstrapCodec {
    /// Encodes a canonical numeric-key, definite-length `Welcome` payload.
    ///
    /// # Errors
    ///
    /// Returns an encoder or bootstrap-size error.
    pub fn encode_welcome(welcome: &Welcome) -> Result<Vec<u8>, BootstrapCodecError> {
        let negotiated = welcome.negotiated();
        let mut encoder = Encoder::new(Vec::with_capacity(256));
        encoder.map(7)?.u8(0)?;
        encode_protocol_version(&mut encoder, negotiated.selected_protocol())?;
        encoder
            .u8(1)?
            .u16(negotiated.selected_catalog_schema().get())?
            .u8(2)?;
        encode_features(&mut encoder, negotiated.enabled_features())?;
        encoder
            .u8(3)?
            .bytes(&welcome.manager_epoch().into_bytes())?
            .u8(4)?
            .bytes(&welcome.connection_id().into_bytes())?
            .u8(5)?;
        encode_authority(&mut encoder, welcome.authority_summary())?;
        encoder.u8(6)?;
        encode_limits(&mut encoder, negotiated.limits())?;

        let bytes = encoder.into_writer();
        if bytes.len() > MAX_BOOTSTRAP_PAYLOAD_BYTES {
            Err(BootstrapCodecError::BootstrapPayloadTooLarge(bytes.len()))
        } else {
            Ok(bytes)
        }
    }

    /// Decodes a bounded version 1 `Welcome` payload.
    ///
    /// # Errors
    ///
    /// Returns a [`BootstrapCodecError`] for malformed, duplicate, missing,
    /// oversized, noncanonical, or trailing data.
    pub fn decode_welcome(bytes: &[u8]) -> Result<Welcome, BootstrapCodecError> {
        if bytes.len() > MAX_BOOTSTRAP_PAYLOAD_BYTES {
            return Err(BootstrapCodecError::BootstrapPayloadTooLarge(bytes.len()));
        }
        let mut decoder = Decoder::new(bytes);
        let field_count = definite_len(decoder.map()?)?;
        if field_count > MAX_BOOTSTRAP_FIELDS {
            return Err(BootstrapCodecError::WrongMapLength(field_count));
        }
        let mut seen = [false; 256];
        let mut selected_protocol = None;
        let mut selected_catalog_schema = None;
        let mut enabled_features = None;
        let mut manager_epoch = None;
        let mut connection_id = None;
        let mut authority_summary = None;
        let mut limits = None;

        for _ in 0..field_count {
            let key = decoder.u8()?;
            mark_seen(&mut seen, key)?;
            match key {
                0 => selected_protocol = Some(decode_protocol_version(&mut decoder)?),
                1 => {
                    selected_catalog_schema = Some(CatalogSchemaVersion::try_from(decoder.u16()?)?);
                }
                2 => enabled_features = Some(decode_features(&mut decoder)?),
                3 => manager_epoch = Some(ManagerEpoch::new(decode_id(&mut decoder)?)?),
                4 => connection_id = Some(ConnectionId::new(decode_id(&mut decoder)?)?),
                5 => authority_summary = Some(decode_authority(&mut decoder)?),
                6 => limits = Some(decode_limits(&mut decoder)?),
                _ => skip_bounded(&mut decoder, 0)?,
            }
        }
        require_fields(&seen, WELCOME_REQUIRED_FIELDS)?;
        if decoder.position() != bytes.len() {
            return Err(BootstrapCodecError::TrailingBytes);
        }

        let negotiated = NegotiatedProtocol::from_selected(
            selected_protocol.ok_or(BootstrapCodecError::MissingKey(0))?,
            selected_catalog_schema.ok_or(BootstrapCodecError::MissingKey(1))?,
            enabled_features.ok_or(BootstrapCodecError::MissingKey(2))?,
            limits.ok_or(BootstrapCodecError::MissingKey(6))?,
        );
        Ok(Welcome::new(
            negotiated,
            manager_epoch.ok_or(BootstrapCodecError::MissingKey(3))?,
            connection_id.ok_or(BootstrapCodecError::MissingKey(4))?,
            authority_summary.ok_or(BootstrapCodecError::MissingKey(5))?,
        ))
    }
}

fn encode_authority(
    encoder: &mut Encoder<Vec<u8>>,
    authority: &AuthoritySummary,
) -> Result<(), BootstrapCodecError> {
    encoder.array(usize_to_u64(authority.capabilities().len())?)?;
    for capability in authority.capabilities() {
        encoder.u16(capability.get())?;
    }
    Ok(())
}

fn decode_authority(decoder: &mut Decoder<'_>) -> Result<AuthoritySummary, BootstrapCodecError> {
    let count = definite_len(decoder.array()?)?;
    let capacity = usize::try_from(count).map_err(|_| BootstrapCodecError::CollectionTooLarge)?;
    if capacity > AuthoritySummary::MAX_CAPABILITIES {
        return Err(crate::AuthoritySummaryError::TooMany(capacity).into());
    }
    let mut capabilities = BTreeSet::new();
    for _ in 0..count {
        let id = AuthorityCapabilityId::try_from(decoder.u16()?)?;
        if !capabilities.insert(id) {
            return Err(BootstrapCodecError::DuplicateAuthorityCapability(id));
        }
    }
    Ok(AuthoritySummary::new(capabilities)?)
}

fn encode_limits(
    encoder: &mut Encoder<Vec<u8>>,
    limits: SessionLimits,
) -> Result<(), BootstrapCodecError> {
    encoder
        .map(6)?
        .u8(0)?
        .u32(limits.frame_payload().get())?
        .u8(1)?
        .u32(limits.control_payload().get())?
        .u8(2)?
        .u32(limits.chunk_payload().get())?
        .u8(3)?
        .u32(limits.reassembly().get())?
        .u8(4)?
        .u8(limits.nesting().get())?
        .u8(5)?
        .u32(limits.assembly_deadline().get())?;
    Ok(())
}

fn decode_limits(decoder: &mut Decoder<'_>) -> Result<SessionLimits, BootstrapCodecError> {
    let field_count = definite_len(decoder.map()?)?;
    if field_count > MAX_BOOTSTRAP_FIELDS {
        return Err(BootstrapCodecError::WrongMapLength(field_count));
    }
    let mut seen = [false; 256];
    let mut frame = None;
    let mut control = None;
    let mut chunk = None;
    let mut reassembly = None;
    let mut nesting = None;
    let mut deadline = None;
    for _ in 0..field_count {
        let key = decoder.u8()?;
        mark_seen(&mut seen, key)?;
        match key {
            0 => frame = Some(FramePayloadLimit::new(decoder.u32()?)?),
            1 => control = Some(ControlPayloadLimit::new(decoder.u32()?)?),
            2 => chunk = Some(ChunkPayloadLimit::new(decoder.u32()?)?),
            3 => reassembly = Some(ReassemblyLimit::new(decoder.u32()?)?),
            4 => nesting = Some(NestingLimit::new(decoder.u8()?)?),
            5 => deadline = Some(AssemblyDeadlineMs::new(decoder.u32()?)?),
            _ => skip_bounded(decoder, 0)?,
        }
    }
    require_fields(&seen, LIMIT_REQUIRED_FIELDS)?;
    Ok(SessionLimits::new(
        frame.ok_or(BootstrapCodecError::MissingKey(0))?,
        control.ok_or(BootstrapCodecError::MissingKey(1))?,
        chunk.ok_or(BootstrapCodecError::MissingKey(2))?,
        reassembly.ok_or(BootstrapCodecError::MissingKey(3))?,
        nesting.ok_or(BootstrapCodecError::MissingKey(4))?,
        deadline.ok_or(BootstrapCodecError::MissingKey(5))?,
    )?)
}

fn decode_id(decoder: &mut Decoder<'_>) -> Result<[u8; 16], BootstrapCodecError> {
    let bytes = decoder.bytes()?;
    bytes
        .try_into()
        .map_err(|_| BootstrapCodecError::WrongIdentifierLength(bytes.len()))
}

fn mark_seen(seen: &mut [bool; 256], key: u8) -> Result<(), BootstrapCodecError> {
    if std::mem::replace(&mut seen[usize::from(key)], true) {
        Err(BootstrapCodecError::DuplicateKey(key))
    } else {
        Ok(())
    }
}

fn require_fields<const N: usize>(
    seen: &[bool; 256],
    required: [u8; N],
) -> Result<(), BootstrapCodecError> {
    for key in required {
        if !seen[usize::from(key)] {
            return Err(BootstrapCodecError::MissingKey(key));
        }
    }
    Ok(())
}
