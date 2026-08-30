use std::num::NonZeroI32;
use std::str::FromStr;

use serde::Deserialize;
use serde::Serialize;
use thiserror::Error;

/// A positive configured window-resize increment in physical pixels.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "i32", into = "i32")]
pub struct ResizeStep(NonZeroI32);

impl ResizeStep {
    /// Creates a positive resize step.
    ///
    /// # Errors
    ///
    /// Returns [`ResizeStepError`] when `value` is zero or negative.
    pub const fn new(value: i32) -> Result<Self, ResizeStepError> {
        match NonZeroI32::new(value) {
            Some(value) if value.is_positive() => Ok(Self(value)),
            _ => Err(ResizeStepError::NonPositive(value)),
        }
    }

    #[must_use]
    pub const fn get(self) -> i32 {
        self.0.get()
    }

    #[must_use]
    pub const fn negative(self) -> i32 {
        -self.get()
    }
}

impl TryFrom<i32> for ResizeStep {
    type Error = ResizeStepError;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<ResizeStep> for i32 {
    fn from(value: ResizeStep) -> Self {
        value.get()
    }
}

impl FromStr for ResizeStep {
    type Err = ResizeStepError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value
            .parse::<i32>()
            .map_err(|_| ResizeStepError::InvalidInteger)?
            .try_into()
    }
}

#[cfg(feature = "schemars")]
impl schemars::JsonSchema for ResizeStep {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("ResizeStep")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "integer",
            "minimum": 1,
            "maximum": 2147483647,
            "description": "A positive window-resize increment in physical pixels."
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum ResizeStepError {
    #[error("resize step must be a base-10 signed 32-bit integer")]
    InvalidInteger,
    #[error("resize step must be positive; received {0}")]
    NonPositive(i32),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resize_step_excludes_zero_negative_and_unrepresentable_values() {
        assert_eq!(ResizeStep::new(50).map(ResizeStep::get), Ok(50));
        assert_eq!(ResizeStep::new(0), Err(ResizeStepError::NonPositive(0)));
        assert_eq!(ResizeStep::new(-1), Err(ResizeStepError::NonPositive(-1)));
        assert_eq!(
            "2147483648".parse::<ResizeStep>(),
            Err(ResizeStepError::InvalidInteger)
        );
    }

    #[test]
    fn serde_keeps_the_integer_shape_and_rejects_invalid_state() {
        let step = ResizeStep::new(50).expect("test step is positive");
        assert_eq!(serde_json::to_string(&step).expect("step serializes"), "50");
        assert_eq!(
            serde_json::from_str::<ResizeStep>("50").expect("positive step deserializes"),
            step
        );
        assert!(serde_json::from_str::<ResizeStep>("0").is_err());
        assert!(serde_json::from_str::<ResizeStep>("-50").is_err());
    }

    #[cfg(feature = "schemars")]
    #[test]
    fn schema_is_a_bounded_integer() {
        let schema = schemars::schema_for!(ResizeStep);
        let value = serde_json::to_value(schema).expect("schema serializes");
        assert_eq!(value["type"], "integer");
        assert_eq!(value["minimum"], 1);
        assert_eq!(value["maximum"], i32::MAX);
    }
}
