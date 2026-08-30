use std::collections::BTreeSet;
use std::num::NonZeroU16;

use super::*;
use crate::CatalogSchemaVersion;
use crate::FeatureId;
use crate::FeatureSet;
use crate::Hello;
use crate::ProtocolMajor;
use crate::ProtocolMinor;
use crate::ProtocolVersion;
use crate::RoleHint;
use crate::VersionRange;
use crate::VersionRanges;

fn protocol(major: u16, minor: u16) -> Result<ProtocolVersion, crate::VersionSetError> {
    Ok(ProtocolVersion::new(
        ProtocolMajor::try_from(major)?,
        ProtocolMinor::new(minor),
    ))
}

fn protocol_ranges(
    first: (u16, u16),
    last: (u16, u16),
) -> Result<VersionRanges<ProtocolVersion>, crate::VersionSetError> {
    VersionRanges::new(vec![VersionRange::new(
        protocol(first.0, first.1)?,
        protocol(last.0, last.1)?,
    )?])
}

fn catalog_ranges(
    first: u16,
    last: u16,
) -> Result<VersionRanges<CatalogSchemaVersion>, crate::VersionSetError> {
    VersionRanges::new(vec![VersionRange::new(
        CatalogSchemaVersion::try_from(first)?,
        CatalogSchemaVersion::try_from(last)?,
    )?])
}

fn features(values: &[u32]) -> Result<FeatureSet, crate::FeatureSetError> {
    FeatureSet::new(
        values
            .iter()
            .copied()
            .map(FeatureId::try_from)
            .collect::<Result<_, _>>()?,
    )
}

#[test]
fn negotiation_selects_highest_common_versions_and_feature_intersection()
-> Result<(), Box<dyn std::error::Error>> {
    let server = ServerSupport::new(
        protocol_ranges((1, 0), (1, 3))?,
        catalog_ranges(1, 2)?,
        features(&[1, 2])?,
        SessionLimits::V1,
    );
    let client = Hello::new(
        protocol_ranges((1, 2), (1, 5))?,
        catalog_ranges(2, 3)?,
        features(&[2, 3])?,
        Some(RoleHint::ExtensionHost),
    );

    let selected = ProtocolNegotiator::select(&server, &client)?;
    assert_eq!(selected.selected_protocol(), protocol(1, 3)?);
    assert_eq!(
        selected.selected_catalog_schema(),
        CatalogSchemaVersion::try_from(2)?
    );
    assert_eq!(selected.enabled_features(), &features(&[2])?);
    assert_eq!(selected.limits(), SessionLimits::V1);
    Ok(())
}

#[test]
fn negotiation_rejects_each_missing_required_overlap() -> Result<(), Box<dyn std::error::Error>> {
    let server = ServerSupport::new(
        protocol_ranges((1, 0), (1, 3))?,
        catalog_ranges(1, 2)?,
        FeatureSet::default(),
        SessionLimits::V1,
    );
    let wrong_protocol = Hello::new(
        protocol_ranges((2, 0), (2, 1))?,
        catalog_ranges(1, 1)?,
        FeatureSet::default(),
        None,
    );
    assert_eq!(
        ProtocolNegotiator::select(&server, &wrong_protocol),
        Err(NegotiationError::UnsupportedProtocol)
    );

    let wrong_catalog = Hello::new(
        protocol_ranges((1, 0), (1, 1))?,
        catalog_ranges(3, 3)?,
        FeatureSet::default(),
        None,
    );
    assert_eq!(
        ProtocolNegotiator::select(&server, &wrong_catalog),
        Err(NegotiationError::UnsupportedCatalogSchema)
    );
    Ok(())
}

#[test]
fn session_limits_reject_zero_ceiling_and_hierarchy_violations() -> Result<(), SessionLimitError> {
    assert_eq!(FramePayloadLimit::new(0), Err(SessionLimitError::Zero));
    assert_eq!(
        SessionLimits::new(
            FramePayloadLimit::new(SessionLimits::V1.frame_payload().get() + 1)?,
            ControlPayloadLimit::new(1)?,
            ChunkPayloadLimit::new(1)?,
            ReassemblyLimit::new(SessionLimits::V1.reassembly().get())?,
            NestingLimit::new(1)?,
            AssemblyDeadlineMs::new(1)?,
        ),
        Err(SessionLimitError::AboveV1Ceiling)
    );
    assert_eq!(
        SessionLimits::new(
            FramePayloadLimit::new(1024)?,
            ControlPayloadLimit::new(2048)?,
            ChunkPayloadLimit::new(512)?,
            ReassemblyLimit::new(4096)?,
            NestingLimit::new(1)?,
            AssemblyDeadlineMs::new(1)?,
        ),
        Err(SessionLimitError::InconsistentPayloadHierarchy)
    );
    Ok(())
}

#[test]
fn identities_reject_nil_and_authority_is_bounded() -> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(ManagerEpoch::new([0; 16]), Err(IdentifierError::Nil));
    assert_eq!(ConnectionId::new([0; 16]), Err(IdentifierError::Nil));

    let capabilities = (1..=128)
        .map(|value| {
            NonZeroU16::new(value)
                .map(AuthorityCapabilityId::new)
                .ok_or(AuthoritySummaryError::TooMany(0))
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    assert_eq!(
        AuthoritySummary::new(capabilities)?.capabilities().len(),
        128
    );
    let too_many = (1..=129)
        .filter_map(NonZeroU16::new)
        .map(AuthorityCapabilityId::new)
        .collect();
    assert_eq!(
        AuthoritySummary::new(too_many),
        Err(AuthoritySummaryError::TooMany(129))
    );
    Ok(())
}
