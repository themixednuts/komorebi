use std::collections::BTreeSet;
use std::convert::Infallible;
use std::num::NonZeroU32;

use thiserror::Error;

use crate::CatalogSchemaVersion;
use crate::FrameKind;
use crate::ProtocolVersion;
use crate::VersionRanges;
use crate::VersionSetError;

mod codec;
mod welcome_codec;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod welcome_tests;

pub const HELLO_FRAME_KIND: FrameKind = FrameKind::new(1);
pub const WELCOME_FRAME_KIND: FrameKind = FrameKind::new(2);
pub const UNSUPPORTED_VERSION_FRAME_KIND: FrameKind = FrameKind::new(3);
pub const PROTOCOL_FAULT_FRAME_KIND: FrameKind = FrameKind::new(4);

const MAX_FEATURES: usize = 256;
const MAX_BOOTSTRAP_PAYLOAD_BYTES: usize = 16 * 1024;
const MAX_BOOTSTRAP_FIELDS: u64 = 32;
const MAX_SKIPPED_COLLECTION_ITEMS: u64 = 1024;
const MAX_NESTING_DEPTH: u8 = 32;
const HELLO_REQUIRED_FIELDS: [u8; 3] = [0, 1, 2];

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct FeatureId(NonZeroU32);

impl FeatureId {
    #[must_use]
    pub const fn new(value: NonZeroU32) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0.get()
    }
}

impl TryFrom<u32> for FeatureId {
    type Error = FeatureSetError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        NonZeroU32::new(value)
            .map(Self)
            .ok_or(FeatureSetError::ZeroFeatureId)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FeatureSet(BTreeSet<FeatureId>);

impl FeatureSet {
    /// Creates a canonical bounded feature set.
    ///
    /// # Errors
    ///
    /// Returns [`FeatureSetError::TooMany`] when the set exceeds the v1 bound.
    pub fn new(features: BTreeSet<FeatureId>) -> Result<Self, FeatureSetError> {
        if features.len() > MAX_FEATURES {
            Err(FeatureSetError::TooMany(features.len()))
        } else {
            Ok(Self(features))
        }
    }

    #[must_use]
    pub fn as_set(&self) -> &BTreeSet<FeatureId> {
        &self.0
    }

    #[must_use]
    pub fn intersection(&self, other: &Self) -> Self {
        Self(self.0.intersection(&other.0).copied().collect())
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum FeatureSetError {
    #[error("feature IDs begin at one")]
    ZeroFeatureId,
    #[error("feature count {0} exceeds the version 1 maximum of {MAX_FEATURES}")]
    TooMany(usize),
}

/// Describes the caller's intended use without granting any authority.
///
/// The transport derives peer identity and permissions from Windows. Receivers
/// must never authorize an operation from this self-declared hint.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum RoleHint {
    OwnerControl = 1,
    FirstPartySurface = 2,
    ExtensionHost = 3,
}

impl RoleHint {
    fn decode(value: u8) -> Result<Self, BootstrapCodecError> {
        match value {
            1 => Ok(Self::OwnerControl),
            2 => Ok(Self::FirstPartySurface),
            3 => Ok(Self::ExtensionHost),
            _ => Err(BootstrapCodecError::UnknownRoleHint(value)),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Hello {
    protocol_versions: VersionRanges<ProtocolVersion>,
    catalog_schemas: VersionRanges<CatalogSchemaVersion>,
    supported_features: FeatureSet,
    requested_role_hint: Option<RoleHint>,
}

impl Hello {
    #[must_use]
    pub fn new(
        protocol_versions: VersionRanges<ProtocolVersion>,
        catalog_schemas: VersionRanges<CatalogSchemaVersion>,
        supported_features: FeatureSet,
        requested_role_hint: Option<RoleHint>,
    ) -> Self {
        Self {
            protocol_versions,
            catalog_schemas,
            supported_features,
            requested_role_hint,
        }
    }

    #[must_use]
    pub const fn protocol_versions(&self) -> &VersionRanges<ProtocolVersion> {
        &self.protocol_versions
    }

    #[must_use]
    pub const fn catalog_schemas(&self) -> &VersionRanges<CatalogSchemaVersion> {
        &self.catalog_schemas
    }

    #[must_use]
    pub const fn supported_features(&self) -> &FeatureSet {
        &self.supported_features
    }

    #[must_use]
    pub const fn requested_role_hint(&self) -> Option<RoleHint> {
        self.requested_role_hint
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct BootstrapCodec;

#[derive(Debug, Error)]
pub enum BootstrapCodecError {
    #[error("bootstrap CBOR collections must use definite lengths")]
    IndefiniteCollection,
    #[error("hello map has {0} fields; bootstrap maximum is {MAX_BOOTSTRAP_FIELDS}")]
    WrongMapLength(u64),
    #[error("hello repeats numeric key {0}")]
    DuplicateKey(u8),
    #[error("hello is missing required numeric key {0}")]
    MissingKey(u8),
    #[error("hello contains trailing bytes")]
    TrailingBytes,
    #[error("bootstrap payload is {0} bytes; maximum is {MAX_BOOTSTRAP_PAYLOAD_BYTES}")]
    BootstrapPayloadTooLarge(usize),
    #[error("version range must contain exactly two entries, received {0}")]
    WrongRangeLength(u64),
    #[error("protocol version must contain major and minor entries, received {0}")]
    WrongProtocolVersionLength(u64),
    #[error("version range count {0} is outside the version 1 bound")]
    InvalidRangeCount(usize),
    #[error("hello repeats feature ID {0:?}")]
    DuplicateFeature(FeatureId),
    #[error("welcome repeats authority capability ID {0:?}")]
    DuplicateAuthorityCapability(crate::AuthorityCapabilityId),
    #[error("session identity must contain exactly 16 bytes, received {0}")]
    WrongIdentifierLength(usize),
    #[error("unknown requested role hint {0}")]
    UnknownRoleHint(u8),
    #[error("collection length is outside the local address space")]
    CollectionTooLarge,
    #[error("unknown bootstrap field exceeds nesting depth {MAX_NESTING_DEPTH}")]
    NestingTooDeep,
    #[error(
        "unknown bootstrap field collection has {0} items; maximum is {MAX_SKIPPED_COLLECTION_ITEMS}"
    )]
    SkippedCollectionTooLarge(u64),
    #[error("unknown bootstrap field contains an unsupported CBOR type")]
    UnsupportedType,
    #[error(transparent)]
    Version(#[from] VersionSetError),
    #[error(transparent)]
    Feature(#[from] FeatureSetError),
    #[error(transparent)]
    Identifier(#[from] crate::IdentifierError),
    #[error(transparent)]
    Authority(#[from] crate::AuthoritySummaryError),
    #[error(transparent)]
    Limits(#[from] crate::SessionLimitError),
    #[error(transparent)]
    Encode(#[from] minicbor::encode::Error<Infallible>),
    #[error(transparent)]
    Decode(#[from] minicbor::decode::Error),
}
