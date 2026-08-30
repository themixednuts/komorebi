use std::str::FromStr;

use serde::Deserialize;
use serde::Serialize;
use thiserror::Error;

macro_rules! signed_border_geometry {
    ($name:ident, $error:ident, $default:expr, $message:literal) => {
        #[derive(
            Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
        )]
        #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
        #[serde(transparent)]
        pub struct $name(i32);

        impl $name {
            pub const DEFAULT: Self = Self($default);

            #[must_use]
            pub const fn new(value: i32) -> Self {
                Self(value)
            }

            #[must_use]
            pub const fn get(self) -> i32 {
                self.0
            }
        }

        impl From<i32> for $name {
            fn from(value: i32) -> Self {
                Self::new(value)
            }
        }

        impl From<$name> for i32 {
            fn from(value: $name) -> Self {
                value.get()
            }
        }

        impl FromStr for $name {
            type Err = $error;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                value.parse::<i32>().map(Self::new).map_err(|_| $error)
            }
        }

        #[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
        #[error($message)]
        pub struct $error;
    };
}

signed_border_geometry!(
    BorderWidth,
    BorderWidthError,
    8,
    "border width must be a base-10 signed 32-bit integer"
);
signed_border_geometry!(
    BorderOffset,
    BorderOffsetError,
    -1,
    "border offset must be a base-10 signed 32-bit integer"
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_geometry_preserves_the_existing_renderer_domain() {
        assert_eq!("-50".parse::<BorderWidth>().map(BorderWidth::get), Ok(-50));
        assert_eq!("50".parse::<BorderOffset>().map(BorderOffset::get), Ok(50));
        assert_eq!(BorderWidth::DEFAULT.get(), 8);
        assert_eq!(BorderOffset::DEFAULT.get(), -1);
    }
}
