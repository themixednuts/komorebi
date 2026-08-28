#![cfg_attr(
    not(test),
    deny(
        clippy::expect_used,
        clippy::panic,
        clippy::unwrap_used,
        unsafe_op_in_unsafe_fn
    )
)]

use std::{fmt, num::NonZeroU64, time::Duration};

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EffectId(NonZeroU64);

impl EffectId {
    pub fn checked(value: u64) -> Result<Self, ModelError> {
        NonZeroU64::new(value)
            .map(Self)
            .ok_or(ModelError::ZeroIdentity)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Generation(NonZeroU64);

impl Generation {
    pub const INITIAL: Self = Self(NonZeroU64::MIN);

    pub fn next(self) -> Result<Self, ModelError> {
        self.0
            .get()
            .checked_add(1)
            .and_then(NonZeroU64::new)
            .map(Self)
            .ok_or(ModelError::GenerationExhausted)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub enum SemanticTarget {
    FocusedWindowOutline,
    WorkspaceAdornment,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rgba {
    pub red: f32,
    pub green: f32,
    pub blue: f32,
    pub alpha: f32,
}

impl Rgba {
    pub fn checked(red: f32, green: f32, blue: f32, alpha: f32) -> Result<Self, ModelError> {
        let value = Self {
            red,
            green,
            blue,
            alpha,
        };
        if [red, green, blue, alpha]
            .into_iter()
            .all(|channel| channel.is_finite() && (0.0..=1.0).contains(&channel))
        {
            Ok(value)
        } else {
            Err(ModelError::InvalidColor)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct BorderParameters {
    pub width_px: f32,
    pub radius_px: f32,
    pub color: Rgba,
    pub pulse_hz: f32,
}

impl BorderParameters {
    pub fn checked(
        width_px: f32,
        radius_px: f32,
        color: Rgba,
        pulse_hz: f32,
    ) -> Result<Self, ModelError> {
        if !(1.0..=16.0).contains(&width_px)
            || !(0.0..=64.0).contains(&radius_px)
            || !(0.0..=4.0).contains(&pulse_hz)
            || !width_px.is_finite()
            || !radius_px.is_finite()
            || !pulse_hz.is_finite()
        {
            return Err(ModelError::ParameterOutOfRange);
        }
        Ok(Self {
            width_px,
            radius_px,
            color,
            pulse_hz,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ParticleParameters {
    pub count: u16,
    pub lifetime_ms: u16,
}

impl ParticleParameters {
    pub fn checked(count: u16, lifetime_ms: u16) -> Result<Self, ModelError> {
        if count > EffectBudget::MAX_PARTICLES || !(16..=5_000).contains(&lifetime_ms) {
            return Err(ModelError::ParameterOutOfRange);
        }
        Ok(Self { count, lifetime_ms })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum EffectParameters {
    FocusBorder(BorderParameters),
    FocusParticles(ParticleParameters),
    WorkspaceAdornment(BorderParameters),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum EffectLifetime {
    WhileTargetIsCurrent,
    Fixed(Duration),
}

impl EffectLifetime {
    pub fn fixed(duration: Duration) -> Result<Self, ModelError> {
        if duration.is_zero() || duration > EffectBudget::MAX_LIFETIME {
            return Err(ModelError::ParameterOutOfRange);
        }
        Ok(Self::Fixed(duration))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EffectBudget {
    pub particles: u16,
    pub texture_bytes: u32,
}

impl EffectBudget {
    pub const MAX_PARTICLES: u16 = 512;
    pub const MAX_TEXTURE_BYTES: u32 = 8 * 1024 * 1024;
    pub const MAX_LIFETIME: Duration = Duration::from_secs(30);

    pub fn checked(particles: u16, texture_bytes: u32) -> Result<Self, ModelError> {
        if particles > Self::MAX_PARTICLES || texture_bytes > Self::MAX_TEXTURE_BYTES {
            return Err(ModelError::BudgetExceeded);
        }
        Ok(Self {
            particles,
            texture_bytes,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EffectInstance {
    pub id: EffectId,
    pub generation: Generation,
    pub target: SemanticTarget,
    pub parameters: EffectParameters,
    pub lifetime: EffectLifetime,
    pub budget: EffectBudget,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EffectLease {
    pub id: EffectId,
    pub generation: Generation,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum EffectCommand {
    Spawn(EffectInstance),
    Update {
        lease: EffectLease,
        parameters: EffectParameters,
    },
    Cancel(EffectLease),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SceneUsage {
    pub instances: u16,
    pub particles: u16,
    pub texture_bytes: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SceneBudget {
    pub instances: u16,
    pub particles: u16,
    pub texture_bytes: u32,
}

impl SceneBudget {
    pub const PERSONAL_PROFILE: Self = Self {
        instances: 64,
        particles: 2_048,
        texture_bytes: 32 * 1024 * 1024,
    };

    pub fn admit(
        self,
        current: SceneUsage,
        requested: EffectBudget,
    ) -> Result<SceneUsage, ModelError> {
        let next = SceneUsage {
            instances: current
                .instances
                .checked_add(1)
                .ok_or(ModelError::BudgetExceeded)?,
            particles: current
                .particles
                .checked_add(requested.particles)
                .ok_or(ModelError::BudgetExceeded)?,
            texture_bytes: current
                .texture_bytes
                .checked_add(requested.texture_bytes)
                .ok_or(ModelError::BudgetExceeded)?,
        };
        if next.instances > self.instances
            || next.particles > self.particles
            || next.texture_bytes > self.texture_bytes
        {
            return Err(ModelError::BudgetExceeded);
        }
        Ok(next)
    }
}

#[derive(Clone, Copy, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AssetDigest([u8; 32]);

impl AssetDigest {
    pub fn of(bytes: &[u8]) -> Self {
        Self(Sha256::digest(bytes).into())
    }

    pub fn verify(self, bytes: &[u8]) -> Result<(), ModelError> {
        (self == Self::of(bytes))
            .then_some(())
            .ok_or(ModelError::DigestMismatch)
    }
}

impl fmt::Debug for AssetDigest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FrameDemand {
    pub generation: Generation,
    pub sequence: NonZeroU64,
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum ModelError {
    #[error("effect identity must be non-zero")]
    ZeroIdentity,
    #[error("effect generation space is exhausted")]
    GenerationExhausted,
    #[error("effect color channels must be finite values in 0..=1")]
    InvalidColor,
    #[error("effect parameter is outside its bounded range")]
    ParameterOutOfRange,
    #[error("effect resource budget exceeds the host limit")]
    BudgetExceeded,
    #[error("shader asset digest does not match its manifest")]
    DigestMismatch,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_identity_color_and_parameters() {
        assert_eq!(EffectId::checked(0), Err(ModelError::ZeroIdentity));
        assert_eq!(
            Rgba::checked(f32::NAN, 0.0, 0.0, 1.0),
            Err(ModelError::InvalidColor)
        );
        let color = Rgba::checked(1.0, 0.2, 0.4, 1.0).unwrap();
        assert_eq!(
            BorderParameters::checked(17.0, 0.0, color, 0.0),
            Err(ModelError::ParameterOutOfRange)
        );
    }

    #[test]
    fn scene_admission_is_pure_and_checked() {
        let request = EffectBudget::checked(512, 8 * 1024 * 1024).unwrap();
        let mut usage = SceneUsage::default();
        for _ in 0..4 {
            usage = SceneBudget::PERSONAL_PROFILE.admit(usage, request).unwrap();
        }
        assert_eq!(
            SceneBudget::PERSONAL_PROFILE.admit(usage, request),
            Err(ModelError::BudgetExceeded)
        );
    }

    #[test]
    fn digest_is_content_addressed() {
        let digest = AssetDigest::of(b"trusted shader");
        assert_eq!(digest.verify(b"trusted shader"), Ok(()));
        assert_eq!(
            digest.verify(b"changed shader"),
            Err(ModelError::DigestMismatch)
        );
    }
}
