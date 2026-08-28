use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::schema::InvocationParameters;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct PrincipalId(String);

impl PrincipalId {
    pub fn parse(value: impl Into<String>) -> Result<Self, InvalidPrincipalId> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 128
            || !value.bytes().all(|byte| byte.is_ascii_graphic())
        {
            return Err(InvalidPrincipalId);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for PrincipalId {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct InvocationId(u64);

impl InvocationId {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn value(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvocationDigest([u8; 32]);

impl InvocationDigest {
    pub fn canonical(parameters: &InvocationParameters) -> Result<Self, serde_json::Error> {
        let encoded = serde_json::to_vec(parameters)?;
        Ok(Self(Sha256::digest(encoded).into()))
    }

    pub const fn bytes(self) -> [u8; 32] {
        self.0
    }

    pub fn from_slice(bytes: &[u8]) -> Result<Self, InvalidDigest> {
        Ok(Self(bytes.try_into().map_err(|_| InvalidDigest)?))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EffectKind {
    IdempotentSetter,
    AmbiguousToggle,
}

impl EffectKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::IdempotentSetter => "idempotent-setter",
            Self::AmbiguousToggle => "ambiguous-toggle",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DurablePhase {
    Reserved,
    LogicalCommitted,
    EffectDispatched,
    Terminal,
}

impl DurablePhase {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Reserved => "reserved",
            Self::LogicalCommitted => "logical-committed",
            Self::EffectDispatched => "effect-dispatched",
            Self::Terminal => "terminal",
        }
    }

    pub fn parse(value: &str) -> Result<Self, InvalidDurablePhase> {
        match value {
            "reserved" => Ok(Self::Reserved),
            "logical-committed" => Ok(Self::LogicalCommitted),
            "effect-dispatched" => Ok(Self::EffectDispatched),
            "terminal" => Ok(Self::Terminal),
            _ => Err(InvalidDurablePhase(value.to_owned())),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryStatus {
    NotReserved,
    RestartedBeforeCommit,
    ReconcilingAfterRestart,
    Indeterminate,
    RetainedTerminal,
    InvocationExpired,
    IdempotencyConflict,
}

#[derive(Clone, Debug)]
pub struct Invocation {
    pub principal: PrincipalId,
    pub id: InvocationId,
    pub digest: InvocationDigest,
    pub parameters: InvocationParameters,
    pub effect: EffectKind,
}

impl Invocation {
    pub fn identity(&self) -> String {
        format!("invocation-{}", self.id.value())
    }
}

pub const MAX_LIVE_INVOCATIONS_PER_PRINCIPAL: usize = 65_536;

#[derive(Debug)]
pub struct AdmissionCapacity {
    live: usize,
}

impl AdmissionCapacity {
    pub const fn empty() -> Self {
        Self { live: 0 }
    }

    pub fn admit(&mut self) -> Result<(), CapacityFull> {
        if self.live >= MAX_LIVE_INVOCATIONS_PER_PRINCIPAL {
            return Err(CapacityFull);
        }
        self.live += 1;
        Ok(())
    }
}

#[derive(Debug, Error)]
#[error("control principal must be 1..=128 printable ASCII bytes")]
pub struct InvalidPrincipalId;

#[derive(Debug, Error)]
#[error("invocation digest must contain exactly 32 bytes")]
pub struct InvalidDigest;

#[derive(Debug, Error)]
#[error("unknown durable invocation phase {0:?}")]
pub struct InvalidDurablePhase(String);

#[derive(Debug, Error)]
#[error("principal already has 65,536 live invocations")]
pub struct CapacityFull;

#[cfg(test)]
mod tests {
    use super::{AdmissionCapacity, MAX_LIVE_INVOCATIONS_PER_PRINCIPAL};

    #[test]
    fn admission_capacity_rejects_one_past_the_contract_limit() {
        let mut capacity = AdmissionCapacity::empty();
        for _ in 0..MAX_LIVE_INVOCATIONS_PER_PRINCIPAL {
            assert!(capacity.admit().is_ok());
        }
        assert!(capacity.admit().is_err());
    }
}
