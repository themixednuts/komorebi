use std::num::NonZeroU16;
use std::num::NonZeroU32;

use proptest::prelude::*;

use super::*;
use crate::VersionRange;

fn hello() -> Result<Hello, BootstrapCodecError> {
    let protocol = ProtocolVersion::new(
        crate::ProtocolMajor::new(NonZeroU16::MIN),
        crate::ProtocolMinor::ZERO,
    );
    let catalog = CatalogSchemaVersion::new(NonZeroU16::MIN);
    Ok(Hello::new(
        VersionRanges::new(vec![VersionRange::new(protocol, protocol)?])?,
        VersionRanges::new(vec![VersionRange::new(catalog, catalog)?])?,
        FeatureSet::new(
            [
                FeatureId::new(NonZeroU32::new(2).ok_or(FeatureSetError::ZeroFeatureId)?),
                FeatureId::new(NonZeroU32::new(7).ok_or(FeatureSetError::ZeroFeatureId)?),
            ]
            .into_iter()
            .collect(),
        )?,
        Some(RoleHint::FirstPartySurface),
    ))
}

proptest! {
    #[test]
    fn arbitrary_bootstrap_payloads_never_panic(bytes in prop::collection::vec(any::<u8>(), 0..4096)) {
        let _ = BootstrapCodec::decode_hello(&bytes);
    }
}

#[test]
fn hello_round_trips_canonically() -> Result<(), Box<dyn std::error::Error>> {
    let hello = hello()?;
    let encoded = BootstrapCodec::encode_hello(&hello)?;
    assert_eq!(
        encoded,
        [
            0xA4, 0x00, 0x81, 0x82, 0x82, 0x01, 0x00, 0x82, 0x01, 0x00, 0x01, 0x81, 0x82, 0x01,
            0x01, 0x02, 0x82, 0x02, 0x07, 0x03, 0x02,
        ]
    );
    assert_eq!(BootstrapCodec::decode_hello(&encoded)?, hello);
    Ok(())
}

#[test]
fn hello_rejects_duplicate_keys_features_and_trailing_bytes()
-> Result<(), Box<dyn std::error::Error>> {
    let source = hello()?;
    let without_role = Hello::new(
        source.protocol_versions.clone(),
        source.catalog_schemas.clone(),
        source.supported_features.clone(),
        None,
    );
    let mut duplicate_key = BootstrapCodec::encode_hello(&without_role)?;
    *duplicate_key
        .first_mut()
        .ok_or(BootstrapCodecError::CollectionTooLarge)? = 0xA4;
    duplicate_key.push(0);
    assert!(matches!(
        BootstrapCodec::decode_hello(&duplicate_key),
        Err(BootstrapCodecError::DuplicateKey(0))
    ));

    let mut duplicate_feature = BootstrapCodec::encode_hello(&without_role)?;
    let last = duplicate_feature
        .last_mut()
        .ok_or(BootstrapCodecError::CollectionTooLarge)?;
    *last = 2;
    let feature_two = FeatureId::try_from(2)?;
    assert!(matches!(
        BootstrapCodec::decode_hello(&duplicate_feature),
        Err(BootstrapCodecError::DuplicateFeature(feature)) if feature == feature_two
    ));

    let mut trailing = BootstrapCodec::encode_hello(&hello()?)?;
    trailing.push(0);
    assert!(matches!(
        BootstrapCodec::decode_hello(&trailing),
        Err(BootstrapCodecError::TrailingBytes)
    ));
    Ok(())
}

#[test]
fn hello_skips_bounded_future_fields_but_rejects_indefinite_values()
-> Result<(), Box<dyn std::error::Error>> {
    let source = hello()?;
    let without_role = Hello::new(
        source.protocol_versions.clone(),
        source.catalog_schemas.clone(),
        source.supported_features.clone(),
        None,
    );
    let mut future = BootstrapCodec::encode_hello(&without_role)?;
    let map_header = future
        .first_mut()
        .ok_or(BootstrapCodecError::CollectionTooLarge)?;
    *map_header = 0xA4;
    future.extend_from_slice(&[4, 0x81, 42]);
    assert_eq!(BootstrapCodec::decode_hello(&future)?, without_role);

    let mut indefinite = BootstrapCodec::encode_hello(&without_role)?;
    let map_header = indefinite
        .first_mut()
        .ok_or(BootstrapCodecError::CollectionTooLarge)?;
    *map_header = 0xA4;
    indefinite.extend_from_slice(&[4, 0x9F, 42, 0xFF]);
    assert!(matches!(
        BootstrapCodec::decode_hello(&indefinite),
        Err(BootstrapCodecError::IndefiniteCollection)
    ));
    Ok(())
}

#[test]
fn hello_bounds_unknown_field_collections_and_nesting() -> Result<(), Box<dyn std::error::Error>> {
    let source = hello()?;
    let without_role = Hello::new(
        source.protocol_versions.clone(),
        source.catalog_schemas.clone(),
        source.supported_features.clone(),
        None,
    );

    let mut oversized = BootstrapCodec::encode_hello(&without_role)?;
    *oversized
        .first_mut()
        .ok_or(BootstrapCodecError::CollectionTooLarge)? = 0xA4;
    oversized.extend_from_slice(&[4, 0x99, 0x04, 0x01]);
    assert!(matches!(
        BootstrapCodec::decode_hello(&oversized),
        Err(BootstrapCodecError::SkippedCollectionTooLarge(1025))
    ));

    let mut nested = BootstrapCodec::encode_hello(&without_role)?;
    *nested
        .first_mut()
        .ok_or(BootstrapCodecError::CollectionTooLarge)? = 0xA4;
    nested.push(4);
    nested.extend(std::iter::repeat_n(0x81, 33));
    nested.push(0);
    assert!(matches!(
        BootstrapCodec::decode_hello(&nested),
        Err(BootstrapCodecError::NestingTooDeep)
    ));
    Ok(())
}
