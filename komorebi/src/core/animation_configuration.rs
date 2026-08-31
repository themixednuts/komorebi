use std::num::NonZeroU64;

use serde::Deserialize;
use serde::Serialize;
use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(transparent)]
pub struct AnimationDuration(u64);

impl AnimationDuration {
    #[must_use]
    pub const fn new(milliseconds: u64) -> Self {
        Self(milliseconds)
    }

    #[must_use]
    pub const fn milliseconds(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(transparent)]
pub struct AnimationFps(NonZeroU64);

impl AnimationFps {
    /// Creates a nonzero animation frame rate.
    ///
    /// # Errors
    ///
    /// Returns [`AnimationFpsError`] when `frames_per_second` is zero.
    pub const fn new(frames_per_second: u64) -> Result<Self, AnimationFpsError> {
        match NonZeroU64::new(frames_per_second) {
            Some(value) => Ok(Self(value)),
            None => Err(AnimationFpsError),
        }
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

impl<'de> Deserialize<'de> for AnimationFps {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Self::new(u64::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[error("animation FPS must be nonzero")]
pub struct AnimationFpsError;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fps_excludes_the_engine_division_by_zero_state() {
        assert_eq!(AnimationFps::new(60).map(AnimationFps::get), Ok(60));
        assert_eq!(AnimationFps::new(0), Err(AnimationFpsError));
        assert!(serde_json::from_str::<AnimationFps>("0").is_err());
    }
}
