use std::collections::BTreeSet;
use std::convert::Infallible;

use thiserror::Error;

use crate::CatalogSchemaVersion;
use crate::FrameKind;
use crate::ProtocolVersion;
use crate::VersionRanges;
use crate::VersionSetError;

mod codec;

#[cfg(test)]
mod tests;

pub const HELLO_FRAME_KIND: FrameKind = FrameKind::new(1);

const MAX_FEATURES: usize = 256;
const MAX_BOOTSTRAP_FIELDS: u64 = 32;
const MAX_SKIPPED_COLLECTION_ITEMS: u64 = 1024;
const MAX_NESTING_DEPTH: u8 = 32;
const HELLO_REQUIRED_FIELDS: [u8; 3] = [0, 1, 2];

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct FeatureId(u16);

impl FeatureId {
    #[must_use]
    pub const fn new(value: u16) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
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
    supported_features: BTreeSet<FeatureId>,
    requested_role_hint: Option<RoleHint>,
}

impl Hello {
    #[must_use]
    pub fn new(
        protocol_versions: VersionRanges<ProtocolVersion>,
        catalog_schemas: VersionRanges<CatalogSchemaVersion>,
        supported_features: BTreeSet<FeatureId>,
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
    pub const fn supported_features(&self) -> &BTreeSet<FeatureId> {
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
    #[error("version range must contain exactly two entries, received {0}")]
    WrongRangeLength(u64),
    #[error("version range count {0} is outside the version 1 bound")]
    InvalidRangeCount(usize),
    #[error("feature count {0} exceeds the version 1 maximum of {MAX_FEATURES}")]
    TooManyFeatures(usize),
    #[error("hello repeats feature ID {0:?}")]
    DuplicateFeature(FeatureId),
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
    Encode(#[from] minicbor::encode::Error<Infallible>),
    #[error(transparent)]
    Decode(#[from] minicbor::decode::Error),
}
