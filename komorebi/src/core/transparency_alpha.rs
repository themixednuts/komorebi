use std::str::FromStr;

use serde::Deserialize;
use serde::Serialize;
use thiserror::Error;

/// An unfocused-window alpha value, where zero is transparent and 255 is opaque.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(transparent)]
pub struct TransparencyAlpha(u8);

impl TransparencyAlpha {
    pub const DEFAULT: Self = Self(200);

    #[must_use]
    pub const fn new(value: u8) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }
}

impl From<u8> for TransparencyAlpha {
    fn from(value: u8) -> Self {
        Self::new(value)
    }
}

impl From<TransparencyAlpha> for u8 {
    fn from(value: TransparencyAlpha) -> Self {
        value.get()
    }
}

impl FromStr for TransparencyAlpha {
    type Err = TransparencyAlphaError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value
            .parse::<u8>()
            .map(Self::new)
            .map_err(|_| TransparencyAlphaError)
    }
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[error("transparency alpha must be a base-10 integer from 0 through 255")]
pub struct TransparencyAlphaError;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alpha_preserves_the_full_byte_domain() {
        assert_eq!(
            "0".parse::<TransparencyAlpha>().map(|value| value.get()),
            Ok(0)
        );
        assert_eq!(
            "255".parse::<TransparencyAlpha>().map(|value| value.get()),
            Ok(255)
        );
        assert_eq!(
            "256".parse::<TransparencyAlpha>(),
            Err(TransparencyAlphaError)
        );
        assert_eq!(
            "-1".parse::<TransparencyAlpha>(),
            Err(TransparencyAlphaError)
        );
    }

    #[test]
    fn serde_preserves_the_bounded_integer_shape() -> Result<(), Box<dyn std::error::Error>> {
        let alpha = TransparencyAlpha::new(177);
        assert_eq!(serde_json::to_string(&alpha)?, "177");
        assert_eq!(serde_json::from_str::<TransparencyAlpha>("177")?, alpha);
        assert!(serde_json::from_str::<TransparencyAlpha>("256").is_err());
        Ok(())
    }
}
