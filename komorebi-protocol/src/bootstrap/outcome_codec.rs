use minicbor::Decoder;
use minicbor::Encoder;

use super::BootstrapCodec;
use super::BootstrapCodecError;
use super::MAX_BOOTSTRAP_FIELDS;
use super::MAX_BOOTSTRAP_PAYLOAD_BYTES;
use super::ProtocolFault;
use super::ProtocolFaultCode;
use super::UnsupportedVersion;
use super::codec::decode_catalog_ranges;
use super::codec::decode_protocol_ranges;
use super::codec::definite_len;
use super::codec::encode_catalog_ranges;
use super::codec::encode_protocol_ranges;
use super::codec::skip_bounded;
use crate::TraceId;

impl BootstrapCodec {
    /// Encodes the server's supported version ranges after negotiation fails.
    ///
    /// # Errors
    ///
    /// Returns an encoder or bootstrap-size error.
    pub fn encode_unsupported_version(
        unsupported: &UnsupportedVersion,
    ) -> Result<Vec<u8>, BootstrapCodecError> {
        let mut encoder = Encoder::new(Vec::with_capacity(128));
        encoder.map(2)?.u8(0)?;
        encode_protocol_ranges(&mut encoder, unsupported.protocol_versions().as_slice())?;
        encoder.u8(1)?;
        encode_catalog_ranges(&mut encoder, unsupported.catalog_schemas().as_slice())?;
        bounded_encoded(encoder.into_writer())
    }

    /// Decodes the bounded `UnsupportedVersion` bootstrap outcome.
    ///
    /// # Errors
    ///
    /// Returns an error for malformed, duplicate, missing, oversized, or
    /// trailing data.
    pub fn decode_unsupported_version(
        bytes: &[u8],
    ) -> Result<UnsupportedVersion, BootstrapCodecError> {
        ensure_bootstrap_bound(bytes)?;
        let mut decoder = Decoder::new(bytes);
        let field_count = bounded_map(&mut decoder)?;
        let mut seen = [false; 256];
        let mut protocol_versions = None;
        let mut catalog_schemas = None;
        for _ in 0..field_count {
            let key = decoder.u8()?;
            mark_seen(&mut seen, key)?;
            match key {
                0 => protocol_versions = Some(decode_protocol_ranges(&mut decoder)?),
                1 => catalog_schemas = Some(decode_catalog_ranges(&mut decoder)?),
                _ => skip_bounded(&mut decoder, 0)?,
            }
        }
        require(&seen, &[0, 1])?;
        require_eof(&decoder, bytes)?;
        Ok(UnsupportedVersion::new(
            protocol_versions.ok_or(BootstrapCodecError::MissingKey(0))?,
            catalog_schemas.ok_or(BootstrapCodecError::MissingKey(1))?,
        ))
    }

    /// Encodes a stable fault code with an opaque trace identity.
    ///
    /// # Errors
    ///
    /// Returns an encoder or bootstrap-size error.
    pub fn encode_protocol_fault(fault: ProtocolFault) -> Result<Vec<u8>, BootstrapCodecError> {
        let mut encoder = Encoder::new(Vec::with_capacity(32));
        encoder
            .map(2)?
            .u8(0)?
            .u16(fault.code() as u16)?
            .u8(1)?
            .bytes(&fault.trace_id().into_bytes())?;
        bounded_encoded(encoder.into_writer())
    }

    /// Decodes a bounded protocol fault without exposing implementation errors.
    ///
    /// # Errors
    ///
    /// Returns an error for malformed, duplicate, missing, oversized, unknown,
    /// or trailing data.
    pub fn decode_protocol_fault(bytes: &[u8]) -> Result<ProtocolFault, BootstrapCodecError> {
        ensure_bootstrap_bound(bytes)?;
        let mut decoder = Decoder::new(bytes);
        let field_count = bounded_map(&mut decoder)?;
        let mut seen = [false; 256];
        let mut code = None;
        let mut trace_id = None;
        for _ in 0..field_count {
            let key = decoder.u8()?;
            mark_seen(&mut seen, key)?;
            match key {
                0 => code = Some(ProtocolFaultCode::decode(decoder.u16()?)?),
                1 => trace_id = Some(TraceId::new(decode_id(&mut decoder)?)?),
                _ => skip_bounded(&mut decoder, 0)?,
            }
        }
        require(&seen, &[0, 1])?;
        require_eof(&decoder, bytes)?;
        Ok(ProtocolFault::new(
            code.ok_or(BootstrapCodecError::MissingKey(0))?,
            trace_id.ok_or(BootstrapCodecError::MissingKey(1))?,
        ))
    }
}

fn bounded_encoded(bytes: Vec<u8>) -> Result<Vec<u8>, BootstrapCodecError> {
    ensure_bootstrap_bound(&bytes)?;
    Ok(bytes)
}

fn ensure_bootstrap_bound(bytes: &[u8]) -> Result<(), BootstrapCodecError> {
    if bytes.len() > MAX_BOOTSTRAP_PAYLOAD_BYTES {
        Err(BootstrapCodecError::BootstrapPayloadTooLarge(bytes.len()))
    } else {
        Ok(())
    }
}

fn bounded_map(decoder: &mut Decoder<'_>) -> Result<u64, BootstrapCodecError> {
    let field_count = definite_len(decoder.map()?)?;
    if field_count > MAX_BOOTSTRAP_FIELDS {
        Err(BootstrapCodecError::WrongMapLength(field_count))
    } else {
        Ok(field_count)
    }
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

fn require(seen: &[bool; 256], required: &[u8]) -> Result<(), BootstrapCodecError> {
    for &key in required {
        if !seen[usize::from(key)] {
            return Err(BootstrapCodecError::MissingKey(key));
        }
    }
    Ok(())
}

fn require_eof(decoder: &Decoder<'_>, bytes: &[u8]) -> Result<(), BootstrapCodecError> {
    if decoder.position() == bytes.len() {
        Ok(())
    } else {
        Err(BootstrapCodecError::TrailingBytes)
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU16;

    use proptest::prelude::*;

    use super::*;
    use crate::CatalogSchemaVersion;
    use crate::ProtocolMajor;
    use crate::ProtocolMinor;
    use crate::ProtocolVersion;
    use crate::VersionRange;
    use crate::VersionRanges;

    fn unsupported() -> Result<UnsupportedVersion, crate::VersionSetError> {
        let protocol =
            ProtocolVersion::new(ProtocolMajor::new(NonZeroU16::MIN), ProtocolMinor::new(3));
        let catalog = CatalogSchemaVersion::new(NonZeroU16::MIN);
        Ok(UnsupportedVersion::new(
            VersionRanges::new(vec![VersionRange::new(protocol, protocol)?])?,
            VersionRanges::new(vec![VersionRange::new(catalog, catalog)?])?,
        ))
    }

    proptest! {
        #[test]
        fn arbitrary_outcome_payloads_never_panic(bytes in prop::collection::vec(any::<u8>(), 0..4096)) {
            let _ = BootstrapCodec::decode_unsupported_version(&bytes);
            let _ = BootstrapCodec::decode_protocol_fault(&bytes);
        }
    }

    #[test]
    fn unsupported_version_round_trips_canonically() -> Result<(), Box<dyn std::error::Error>> {
        let value = unsupported()?;
        let bytes = BootstrapCodec::encode_unsupported_version(&value)?;
        assert_eq!(
            bytes,
            [
                0xA2, 0x00, 0x81, 0x82, 0x82, 0x01, 0x03, 0x82, 0x01, 0x03, 0x01, 0x81, 0x82, 0x01,
                0x01,
            ]
        );
        assert_eq!(BootstrapCodec::decode_unsupported_version(&bytes)?, value);
        Ok(())
    }

    #[test]
    fn protocol_fault_round_trips_canonically() -> Result<(), Box<dyn std::error::Error>> {
        let fault =
            ProtocolFault::new(ProtocolFaultCode::SequenceViolation, TraceId::new([9; 16])?);
        let bytes = BootstrapCodec::encode_protocol_fault(fault)?;
        let mut fixture = vec![0xA2, 0x00, 0x03, 0x01, 0x50];
        fixture.extend_from_slice(&[9; 16]);
        assert_eq!(bytes, fixture);
        assert_eq!(BootstrapCodec::decode_protocol_fault(&bytes)?, fault);
        Ok(())
    }
}
