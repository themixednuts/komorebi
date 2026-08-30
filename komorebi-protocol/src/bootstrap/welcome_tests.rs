use std::num::NonZeroU16;

use proptest::prelude::*;

use super::*;
use crate::AuthorityCapabilityId;
use crate::AuthoritySummary;
use crate::ConnectionId;
use crate::FeatureSet;
use crate::IdentifierError;
use crate::ManagerEpoch;
use crate::NegotiatedProtocol;
use crate::ProtocolMajor;
use crate::ProtocolMinor;
use crate::SessionLimits;
use crate::Welcome;

fn welcome() -> Result<Welcome, Box<dyn std::error::Error>> {
    let negotiated = NegotiatedProtocol::from_selected(
        ProtocolVersion::new(ProtocolMajor::try_from(1)?, ProtocolMinor::new(3)),
        CatalogSchemaVersion::try_from(2)?,
        FeatureSet::new([FeatureId::try_from(7)?].into_iter().collect())?,
        SessionLimits::V1,
    );
    Ok(Welcome::new(
        negotiated,
        ManagerEpoch::new([1; 16])?,
        ConnectionId::new([2; 16])?,
        AuthoritySummary::new(
            [AuthorityCapabilityId::new(NonZeroU16::MIN)]
                .into_iter()
                .collect(),
        )?,
    ))
}

proptest! {
    #[test]
    fn arbitrary_welcome_payloads_never_panic(bytes in prop::collection::vec(any::<u8>(), 0..4096)) {
        let _ = BootstrapCodec::decode_welcome(&bytes);
    }
}

#[test]
fn welcome_round_trips_canonically() -> Result<(), Box<dyn std::error::Error>> {
    let welcome = welcome()?;
    let encoded = BootstrapCodec::encode_welcome(&welcome)?;
    let mut fixture = vec![
        0xA7, 0x00, 0x82, 0x01, 0x03, 0x01, 0x02, 0x02, 0x81, 0x07, 0x03, 0x50,
    ];
    fixture.extend_from_slice(&[1; 16]);
    fixture.extend_from_slice(&[0x04, 0x50]);
    fixture.extend_from_slice(&[2; 16]);
    fixture.extend_from_slice(&[
        0x05, 0x81, 0x01, 0x06, 0xA6, 0x00, 0x1A, 0x00, 0x10, 0x00, 0x00, 0x01, 0x19, 0x40, 0x00,
        0x02, 0x1A, 0x00, 0x01, 0x00, 0x00, 0x03, 0x1A, 0x00, 0x80, 0x00, 0x00, 0x04, 0x18, 0x20,
        0x05, 0x19, 0x07, 0xD0,
    ]);
    assert_eq!(encoded, fixture);
    let decoded = BootstrapCodec::decode_welcome(&encoded)?;
    assert_eq!(decoded, welcome);
    assert_eq!(BootstrapCodec::encode_welcome(&decoded)?, encoded);
    Ok(())
}

#[test]
fn welcome_rejects_nil_identity_and_oversized_payload() -> Result<(), Box<dyn std::error::Error>> {
    let mut encoded = BootstrapCodec::encode_welcome(&welcome()?)?;
    let epoch_offset = encoded
        .windows(16)
        .position(|window| window == [1; 16])
        .ok_or(BootstrapCodecError::WrongIdentifierLength(0))?;
    encoded[epoch_offset..epoch_offset + 16].fill(0);
    assert!(matches!(
        BootstrapCodec::decode_welcome(&encoded),
        Err(BootstrapCodecError::Identifier(IdentifierError::Nil))
    ));

    let oversized = vec![0; MAX_BOOTSTRAP_PAYLOAD_BYTES + 1];
    assert!(matches!(
        BootstrapCodec::decode_welcome(&oversized),
        Err(BootstrapCodecError::BootstrapPayloadTooLarge(_))
    ));
    Ok(())
}
