use std::collections::BTreeSet;
use std::num::NonZeroU16;

use thiserror::Error;

const MAX_AUTHORITY_CAPABILITIES: usize = 128;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct AuthorityCapabilityId(NonZeroU16);

impl AuthorityCapabilityId {
    #[must_use]
    pub const fn new(value: NonZeroU16) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u16 {
        self.0.get()
    }
}

impl TryFrom<u16> for AuthorityCapabilityId {
    type Error = AuthoritySummaryError;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        NonZeroU16::new(value)
            .map(Self)
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
