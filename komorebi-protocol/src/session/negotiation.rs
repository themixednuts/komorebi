use thiserror::Error;

use crate::CatalogSchemaVersion;
use crate::FeatureSet;
use crate::Hello;
use crate::ProtocolVersion;
use crate::VersionRanges;

use super::SessionLimits;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServerSupport {
    protocol_versions: VersionRanges<ProtocolVersion>,
    catalog_schemas: VersionRanges<CatalogSchemaVersion>,
    features: FeatureSet,
    limits: SessionLimits,
}

impl ServerSupport {
    #[must_use]
    pub fn new(
        protocol_versions: VersionRanges<ProtocolVersion>,
        catalog_schemas: VersionRanges<CatalogSchemaVersion>,
        features: FeatureSet,
        limits: SessionLimits,
    ) -> Self {
        Self {
            protocol_versions,
            catalog_schemas,
            features,
            limits,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NegotiatedProtocol {
    selected_protocol: ProtocolVersion,
    selected_catalog_schema: CatalogSchemaVersion,
    enabled_features: FeatureSet,
    limits: SessionLimits,
}

impl NegotiatedProtocol {
    pub(crate) fn from_selected(
        selected_protocol: ProtocolVersion,
        selected_catalog_schema: CatalogSchemaVersion,
        enabled_features: FeatureSet,
        limits: SessionLimits,
    ) -> Self {
        Self {
            selected_protocol,
            selected_catalog_schema,
            enabled_features,
            limits,
        }
    }

    #[must_use]
    pub const fn selected_protocol(&self) -> ProtocolVersion {
        self.selected_protocol
    }

    #[must_use]
    pub const fn selected_catalog_schema(&self) -> CatalogSchemaVersion {
        self.selected_catalog_schema
    }

    #[must_use]
    pub const fn enabled_features(&self) -> &FeatureSet {
        &self.enabled_features
    }

    #[must_use]
    pub const fn limits(&self) -> SessionLimits {
        self.limits
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ProtocolNegotiator;

impl ProtocolNegotiator {
    /// Selects the highest common protocol and catalog schema versions.
    ///
    /// # Errors
    ///
    /// Returns [`NegotiationError`] when either required version family has no
    /// overlap. A role hint is deliberately absent from this decision.
    pub fn select(
        server: &ServerSupport,
        client: &Hello,
    ) -> Result<NegotiatedProtocol, NegotiationError> {
        let selected_protocol = server
            .protocol_versions
            .highest_common(client.protocol_versions())
            .ok_or(NegotiationError::UnsupportedProtocol)?;
        let selected_catalog_schema = server
            .catalog_schemas
            .highest_common(client.catalog_schemas())
            .ok_or(NegotiationError::UnsupportedCatalogSchema)?;
        Ok(NegotiatedProtocol::from_selected(
            selected_protocol,
            selected_catalog_schema,
            server.features.intersection(client.supported_features()),
            server.limits,
        ))
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum NegotiationError {
    #[error("client and server have no common protocol version")]
    UnsupportedProtocol,
    #[error("client and server have no common catalog schema version")]
    UnsupportedCatalogSchema,
}
