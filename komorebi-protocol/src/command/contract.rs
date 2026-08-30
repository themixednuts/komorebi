use std::fmt;
use std::num::NonZeroU16;
use std::num::NonZeroU64;

use thiserror::Error;

use super::ActionArguments;
use super::StableIdError;
use super::argument::validate_stable_id;
use crate::IdentifierError;
use crate::InvocationId;
use crate::ManagerEpoch;

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ActionId(Box<str>);

impl ActionId {
    /// Creates a bounded stable action identifier.
    ///
    /// # Errors
    ///
    /// Returns [`StableIdError`] for empty, oversized, or malformed input.
    pub fn parse(value: impl Into<Box<str>>) -> Result<Self, StableIdError> {
        let value = value.into();
        validate_stable_id(&value, "action ID")?;
        Ok(Self(value))
    }

    pub(super) fn from_known(value: &'static str) -> Self {
        Self(value.into())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ActionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ActionSchemaVersion(NonZeroU16);

impl ActionSchemaVersion {
    #[must_use]
    pub const fn new(value: NonZeroU16) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u16 {
        self.0.get()
    }
}

impl TryFrom<u16> for ActionSchemaVersion {
    type Error = ActionContractError;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        NonZeroU16::new(value)
            .map(Self)
            .ok_or(ActionContractError::ZeroActionSchemaVersion)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Revision(NonZeroU64);

impl Revision {
    pub const FIRST: Self = Self(NonZeroU64::MIN);

    #[must_use]
    pub const fn new(value: NonZeroU64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0.get()
    }

    /// Advances the revision without wrapping the authoritative sequence.
    ///
    /// # Errors
    ///
    /// Returns [`ActionContractError::RevisionExhausted`] at `u64::MAX`.
    pub const fn next(self) -> Result<Self, ActionContractError> {
        match self.0.checked_add(1) {
            Some(value) => Ok(Self(value)),
            None => Err(ActionContractError::RevisionExhausted),
        }
    }
}

impl TryFrom<u64> for Revision {
    type Error = ActionContractError;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        NonZeroU64::new(value)
            .map(Self)
            .ok_or(ActionContractError::ZeroRevision)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct ActionContractFingerprint([u8; 32]);

impl ActionContractFingerprint {
    #[must_use]
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub const fn into_bytes(self) -> [u8; 32] {
        self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ActionKey {
    id: ActionId,
    schema_version: ActionSchemaVersion,
}

impl ActionKey {
    #[must_use]
    pub const fn new(id: ActionId, schema_version: ActionSchemaVersion) -> Self {
        Self { id, schema_version }
    }

    #[must_use]
    pub const fn id(&self) -> &ActionId {
        &self.id
    }

    #[must_use]
    pub const fn schema_version(&self) -> ActionSchemaVersion {
        self.schema_version
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct StateStamp {
    epoch: ManagerEpoch,
    revision: Revision,
}

impl StateStamp {
    #[must_use]
    pub const fn initial(epoch: ManagerEpoch) -> Self {
        Self::new(epoch, Revision::FIRST)
    }

    #[must_use]
    pub const fn new(epoch: ManagerEpoch, revision: Revision) -> Self {
        Self { epoch, revision }
    }

    #[must_use]
    pub const fn epoch(self) -> ManagerEpoch {
        self.epoch
    }

    #[must_use]
    pub const fn revision(self) -> Revision {
        self.revision
    }

    /// Advances this manager's state sequence without changing its epoch.
    ///
    /// # Errors
    ///
    /// Returns [`ActionContractError::RevisionExhausted`] at `u64::MAX`.
    pub const fn next(self) -> Result<Self, ActionContractError> {
        match self.revision.next() {
            Ok(revision) => Ok(Self::new(self.epoch, revision)),
            Err(error) => Err(error),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct CatalogStamp {
    epoch: ManagerEpoch,
    definition_revision: Revision,
    offer_revision: Revision,
    grant_revision: Revision,
}

impl CatalogStamp {
    #[must_use]
    pub const fn new(
        epoch: ManagerEpoch,
        definition_revision: Revision,
        offer_revision: Revision,
        grant_revision: Revision,
    ) -> Self {
        Self {
            epoch,
            definition_revision,
            offer_revision,
            grant_revision,
        }
    }

    #[must_use]
    pub const fn epoch(self) -> ManagerEpoch {
        self.epoch
    }

    #[must_use]
    pub const fn definition_revision(self) -> Revision {
        self.definition_revision
    }

    #[must_use]
    pub const fn offer_revision(self) -> Revision {
        self.offer_revision
    }

    #[must_use]
    pub const fn grant_revision(self) -> Revision {
        self.grant_revision
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct OfferRef {
    action: ActionKey,
    contract: ActionContractFingerprint,
    catalog: CatalogStamp,
}

impl OfferRef {
    #[must_use]
    pub const fn new(
        action: ActionKey,
        contract: ActionContractFingerprint,
        catalog: CatalogStamp,
    ) -> Self {
        Self {
            action,
            contract,
            catalog,
        }
    }

    #[must_use]
    pub const fn action(&self) -> &ActionKey {
        &self.action
    }

    #[must_use]
    pub const fn contract(&self) -> ActionContractFingerprint {
        self.contract
    }

    #[must_use]
    pub const fn catalog(&self) -> CatalogStamp {
        self.catalog
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct ConfirmationChallengeId([u8; 16]);

impl ConfirmationChallengeId {
    /// Creates a non-nil confirmation challenge identity.
    ///
    /// # Errors
    ///
    /// Returns [`IdentifierError::Nil`] for an all-zero value.
    pub fn new(bytes: [u8; 16]) -> Result<Self, IdentifierError> {
        if bytes == [0; 16] {
            Err(IdentifierError::Nil)
        } else {
            Ok(Self(bytes))
        }
    }

    #[must_use]
    pub const fn into_bytes(self) -> [u8; 16] {
        self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActionInvocation {
    invocation_id: InvocationId,
    offer: OfferRef,
    expected_state: StateStamp,
    arguments: ActionArguments,
    confirmation: Option<ConfirmationChallengeId>,
}

impl ActionInvocation {
    #[must_use]
    pub const fn new(
        invocation_id: InvocationId,
        offer: OfferRef,
        expected_state: StateStamp,
        arguments: ActionArguments,
        confirmation: Option<ConfirmationChallengeId>,
    ) -> Self {
        Self {
            invocation_id,
            offer,
            expected_state,
            arguments,
            confirmation,
        }
    }

    #[must_use]
    pub const fn invocation_id(&self) -> InvocationId {
        self.invocation_id
    }

    #[must_use]
    pub const fn offer(&self) -> &OfferRef {
        &self.offer
    }

    #[must_use]
    pub const fn expected_state(&self) -> StateStamp {
        self.expected_state
    }

    #[must_use]
    pub const fn arguments(&self) -> &ActionArguments {
        &self.arguments
    }

    #[must_use]
    pub const fn confirmation(&self) -> Option<ConfirmationChallengeId> {
        self.confirmation
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum ActionContractError {
    #[error("action schema versions begin at one")]
    ZeroActionSchemaVersion,
    #[error("revisions begin at one")]
    ZeroRevision,
    #[error("revision sequence exhausted")]
    RevisionExhausted,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revision_advances_without_zero_or_wraparound() -> Result<(), ActionContractError> {
        let first = Revision::try_from(1)?;
        assert_eq!(first.next()?.get(), 2);

        let last = Revision::try_from(u64::MAX)?;
        assert_eq!(last.next(), Err(ActionContractError::RevisionExhausted));
        Ok(())
    }

    #[test]
    fn state_stamp_advances_inside_one_epoch() -> Result<(), Box<dyn std::error::Error>> {
        let epoch = ManagerEpoch::new([1; 16])?;
        let initial = StateStamp::initial(epoch);
        let next = initial.next()?;
        assert_eq!(initial.revision(), Revision::FIRST);
        assert_eq!(next.epoch(), epoch);
        assert_eq!(next.revision().get(), 2);
        Ok(())
    }
}
