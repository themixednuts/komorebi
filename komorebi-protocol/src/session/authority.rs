use std::collections::BTreeSet;
use std::num::NonZeroU16;

use thiserror::Error;

const MAX_AUTHORITY_CAPABILITIES: usize = 128;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct AuthorityCapabilityId(u16);

impl AuthorityCapabilityId {
    #[must_use]
    pub const fn new(value: NonZeroU16) -> Self {
        Self(value.get())
    }

    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
#[repr(u16)]
pub enum CommandCapability {
    ReadCatalog = 1,
    InvokeActions = 2,
    ControlOwnInvocations = 3,
    SubscribeEvents = 4,
}

impl CommandCapability {
    pub const ALL: [Self; 4] = [
        Self::ReadCatalog,
        Self::InvokeActions,
        Self::ControlOwnInvocations,
        Self::SubscribeEvents,
    ];

    #[must_use]
    pub const fn id(self) -> AuthorityCapabilityId {
        AuthorityCapabilityId(self as u16)
    }
}

impl TryFrom<u16> for AuthorityCapabilityId {
    type Error = AuthoritySummaryError;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        NonZeroU16::new(value)
            .map(Self::new)
            .ok_or(AuthoritySummaryError::ZeroCapabilityId)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AuthoritySummary(BTreeSet<AuthorityCapabilityId>);

impl AuthoritySummary {
    pub const MAX_CAPABILITIES: usize = MAX_AUTHORITY_CAPABILITIES;

    /// Creates a bounded summary of server-issued authority.
    ///
    /// # Errors
    ///
    /// Returns [`AuthoritySummaryError::TooMany`] when the summary is oversized.
    pub fn new(
        capabilities: BTreeSet<AuthorityCapabilityId>,
    ) -> Result<Self, AuthoritySummaryError> {
        if capabilities.len() > MAX_AUTHORITY_CAPABILITIES {
            Err(AuthoritySummaryError::TooMany(capabilities.len()))
        } else {
            Ok(Self(capabilities))
        }
    }

    #[must_use]
    pub fn command_owner() -> Self {
        Self(
            CommandCapability::ALL
                .into_iter()
                .map(CommandCapability::id)
                .collect(),
        )
    }

    #[must_use]
    pub fn permits(&self, capability: CommandCapability) -> bool {
        self.0.contains(&capability.id())
    }

    #[must_use]
    pub fn capabilities(&self) -> &BTreeSet<AuthorityCapabilityId> {
        &self.0
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum AuthoritySummaryError {
    #[error("authority capability IDs begin at one")]
    ZeroCapabilityId,
    #[error("authority summary has {0} capabilities; maximum is {MAX_AUTHORITY_CAPABILITIES}")]
    TooMany(usize),
}
