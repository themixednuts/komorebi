use std::collections::BTreeMap;
use std::str::FromStr;

use thiserror::Error;

const MAX_STABLE_ID_BYTES: usize = 128;
const MAX_TEXT_BYTES: usize = 4096;
const MAX_PATH_UNITS: usize = 4096;
pub(crate) const MAX_ARGUMENTS: usize = 64;
pub(crate) const MAX_LIST_ITEMS: usize = 256;
const MAX_DECIMAL_SCALE: u8 = 18;

macro_rules! stable_id {
    ($name:ident, $label:literal) => {
        #[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
        pub struct $name(Box<str>);

        impl $name {
            /// Creates a bounded lowercase ASCII identifier.
            ///
            /// # Errors
            ///
            /// Returns [`StableIdError`] for empty, oversized, or malformed input.
            pub fn parse(value: impl Into<Box<str>>) -> Result<Self, StableIdError> {
                let value = value.into();
                validate_stable_id(&value, $label)?;
                Ok(Self(value))
            }

            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
    };
}

stable_id!(ParameterId, "parameter ID");
stable_id!(ChoiceId, "choice ID");
stable_id!(EntityKind, "entity kind");
stable_id!(EntityId, "entity ID");
stable_id!(SelectorId, "selector ID");

impl ParameterId {
    pub(super) fn from_known(value: &'static str) -> Self {
        Self(value.into())
    }
}

impl ChoiceId {
    pub(super) fn from_known(value: &'static str) -> Self {
        Self(value.into())
    }
}

impl SelectorId {
    pub(super) fn from_known(value: &'static str) -> Self {
        Self(value.into())
    }
}

pub(crate) fn validate_stable_id(value: &str, label: &'static str) -> Result<(), StableIdError> {
    if value.is_empty() {
        return Err(StableIdError::Empty(label));
    }
    if value.len() > MAX_STABLE_ID_BYTES {
        return Err(StableIdError::TooLong {
            label,
            actual: value.len(),
        });
    }
    if !value.bytes().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'.')
    }) {
        return Err(StableIdError::InvalidCharacter(label));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum StableIdError {
    #[error("{0} must not be empty")]
    Empty(&'static str),
    #[error("{label} has {actual} bytes; maximum is {MAX_STABLE_ID_BYTES}")]
    TooLong { label: &'static str, actual: usize },
    #[error("{0} must contain only lowercase ASCII letters, digits, '-' or '.'")]
    InvalidCharacter(&'static str),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundedText(Box<str>);

impl BoundedText {
    /// Creates bounded UTF-8 text.
    ///
    /// # Errors
    ///
    /// Returns [`ArgumentError::TextTooLong`] above the protocol bound.
    pub fn new(value: impl Into<Box<str>>) -> Result<Self, ArgumentError> {
        let value = value.into();
        if value.len() > MAX_TEXT_BYTES {
            Err(ArgumentError::TextTooLong(value.len()))
        } else {
            Ok(Self(value))
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Lossless Windows path input represented as native UTF-16 code units.
///
/// Unpaired surrogates are preserved. Interior NUL is rejected because no
/// Windows path API can consume it as part of a path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WindowsPathInput(Box<[u16]>);

impl WindowsPathInput {
    /// Creates bounded, NUL-free Windows path input without Unicode repair.
    ///
    /// # Errors
    ///
    /// Returns [`ArgumentError`] for empty, oversized, or NUL-containing input.
    pub fn new(units: impl Into<Box<[u16]>>) -> Result<Self, ArgumentError> {
        let units = units.into();
        if units.is_empty() {
            return Err(ArgumentError::EmptyPath);
        }
        if units.len() > MAX_PATH_UNITS {
            return Err(ArgumentError::PathTooLong(units.len()));
        }
        if units.contains(&0) {
            return Err(ArgumentError::PathContainsNul);
        }
        Ok(Self(units))
    }

    #[must_use]
    pub fn units(&self) -> &[u16] {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FixedDecimal {
    coefficient: i64,
    scale: u8,
}

impl FixedDecimal {
    /// Creates an exact base-10 value `coefficient * 10^-scale`.
    ///
    /// # Errors
    ///
    /// Returns [`ArgumentError::DecimalScaleTooLarge`] above 18 places.
    pub const fn new(coefficient: i64, scale: u8) -> Result<Self, ArgumentError> {
        if scale > MAX_DECIMAL_SCALE {
            Err(ArgumentError::DecimalScaleTooLarge(scale))
        } else {
            Ok(Self { coefficient, scale })
        }
    }

    #[must_use]
    pub const fn coefficient(self) -> i64 {
        self.coefficient
    }

    #[must_use]
    pub const fn scale(self) -> u8 {
        self.scale
    }
}

impl FromStr for FixedDecimal {
    type Err = ArgumentError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (negative, unsigned) = value
            .strip_prefix('-')
            .map_or((false, value), |value| (true, value));
        let mut parts = unsigned.split('.');
        let integer = parts.next().ok_or(ArgumentError::InvalidDecimalText)?;
        let fraction = parts.next();
        if parts.next().is_some()
            || integer.is_empty()
            || fraction.is_some_and(str::is_empty)
            || !integer.bytes().all(|byte| byte.is_ascii_digit())
            || fraction.is_some_and(|digits| !digits.bytes().all(|byte| byte.is_ascii_digit()))
        {
            return Err(ArgumentError::InvalidDecimalText);
        }
        let scale = fraction.map_or(0, str::len);
        if scale > usize::from(MAX_DECIMAL_SCALE) {
            return Err(ArgumentError::DecimalTextScaleTooLarge(scale));
        }
        let mut coefficient = 0_i128;
        for digit in integer.bytes().chain(fraction.unwrap_or("").bytes()) {
            coefficient = coefficient
                .checked_mul(10)
                .and_then(|value| value.checked_add(i128::from(digit - b'0')))
                .ok_or(ArgumentError::DecimalCoefficientOutOfRange)?;
        }
        if negative {
            coefficient = -coefficient;
        }
        let coefficient =
            i64::try_from(coefficient).map_err(|_| ArgumentError::DecimalCoefficientOutOfRange)?;
        Self::new(
            coefficient,
            u8::try_from(scale).map_err(|_| ArgumentError::DecimalTextScaleTooLarge(scale))?,
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Color([u16; 4]);

impl Color {
    #[must_use]
    pub const fn new(red: u16, green: u16, blue: u16, alpha: u16) -> Self {
        Self([red, green, blue, alpha])
    }

    #[must_use]
    pub const fn channels(self) -> [u16; 4] {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Unit {
    Pixels = 1,
    BasisPoints = 2,
    Milliseconds = 3,
}

impl Unit {
    pub(crate) fn decode(value: u8) -> Result<Self, ArgumentError> {
        match value {
            1 => Ok(Self::Pixels),
            2 => Ok(Self::BasisPoints),
            3 => Ok(Self::Milliseconds),
            _ => Err(ArgumentError::UnknownUnit(value)),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnitValue {
    unit: Unit,
    magnitude: i64,
}

impl UnitValue {
    #[must_use]
    pub const fn new(unit: Unit, magnitude: i64) -> Self {
        Self { unit, magnitude }
    }

    #[must_use]
    pub const fn unit(self) -> Unit {
        self.unit
    }

    #[must_use]
    pub const fn magnitude(self) -> i64 {
        self.magnitude
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EntityReference {
    kind: EntityKind,
    id: EntityId,
}

impl EntityReference {
    #[must_use]
    pub const fn new(kind: EntityKind, id: EntityId) -> Self {
        Self { kind, id }
    }

    #[must_use]
    pub const fn kind(&self) -> &EntityKind {
        &self.kind
    }

    #[must_use]
    pub const fn id(&self) -> &EntityId {
        &self.id
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ArgumentScalar {
    Bool(bool),
    Signed(i64),
    Unsigned(u64),
    Decimal(FixedDecimal),
    Text(BoundedText),
    Choice(ChoiceId),
    Color(Color),
    Unit(UnitValue),
    Entity(EntityReference),
    Selector(SelectorId),
    WindowsPath(WindowsPathInput),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ActionArgument {
    Scalar(ArgumentScalar),
    Scalars(ArgumentScalars),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArgumentScalars(Box<[ArgumentScalar]>);

impl ArgumentScalars {
    /// Creates a bounded, nonempty flat scalar list.
    ///
    /// # Errors
    ///
    /// Returns [`ArgumentError`] when the list is empty or exceeds 256 items.
    pub fn new(values: impl Into<Box<[ArgumentScalar]>>) -> Result<Self, ArgumentError> {
        let values = values.into();
        if values.is_empty() {
            return Err(ArgumentError::EmptyList);
        }
        if values.len() > MAX_LIST_ITEMS {
            return Err(ArgumentError::TooManyListItems(values.len()));
        }
        Ok(Self(values))
    }

    #[must_use]
    pub fn values(&self) -> &[ArgumentScalar] {
        &self.0
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ActionArguments(BTreeMap<ParameterId, ActionArgument>);

impl ActionArguments {
    /// Creates a canonical bounded parameter map.
    ///
    /// # Errors
    ///
    /// Returns [`ArgumentError::TooManyArguments`] above 64 entries.
    pub fn new(values: BTreeMap<ParameterId, ActionArgument>) -> Result<Self, ArgumentError> {
        if values.len() > MAX_ARGUMENTS {
            Err(ArgumentError::TooManyArguments(values.len()))
        } else {
            Ok(Self(values))
        }
    }

    #[must_use]
    pub fn values(&self) -> &BTreeMap<ParameterId, ActionArgument> {
        &self.0
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ArgumentError {
    #[error("text has {0} UTF-8 bytes; maximum is {MAX_TEXT_BYTES}")]
    TextTooLong(usize),
    #[error("Windows path input must not be empty")]
    EmptyPath,
    #[error("Windows path input has {0} UTF-16 units; maximum is {MAX_PATH_UNITS}")]
    PathTooLong(usize),
    #[error("Windows path input contains an interior NUL")]
    PathContainsNul,
    #[error("decimal scale {0} exceeds {MAX_DECIMAL_SCALE}")]
    DecimalScaleTooLarge(u8),
    #[error("decimal text must contain an optional '-' and base-10 digits with one optional point")]
    InvalidDecimalText,
    #[error("decimal text has {0} fractional digits; maximum is {MAX_DECIMAL_SCALE}")]
    DecimalTextScaleTooLarge(usize),
    #[error("decimal coefficient does not fit a signed 64-bit value")]
    DecimalCoefficientOutOfRange,
    #[error("unknown unit ID {0}")]
    UnknownUnit(u8),
    #[error("scalar lists must not be empty")]
    EmptyList,
    #[error("scalar list has {0} entries; maximum is {MAX_LIST_ITEMS}")]
    TooManyListItems(usize),
    #[error("action has {0} arguments; maximum is {MAX_ARGUMENTS}")]
    TooManyArguments(usize),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimal_text_parsing_is_exact_and_bounded() -> Result<(), ArgumentError> {
        let value = "0.125".parse::<FixedDecimal>()?;
        assert_eq!(value.coefficient(), 125);
        assert_eq!(value.scale(), 3);
        assert_eq!(
            "1.2.3".parse::<FixedDecimal>(),
            Err(ArgumentError::InvalidDecimalText)
        );
        assert_eq!(
            "0.1234567890123456789".parse::<FixedDecimal>(),
            Err(ArgumentError::DecimalTextScaleTooLarge(19))
        );
        Ok(())
    }
}
