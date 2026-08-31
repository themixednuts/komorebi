use komorebi_protocol::ActionArgument;
use komorebi_protocol::ArgumentCardinality;
use komorebi_protocol::ArgumentError;
use komorebi_protocol::ArgumentScalar;
use komorebi_protocol::ArgumentScalars;
use komorebi_protocol::BoundedText;
use komorebi_protocol::ChoiceId;
use komorebi_protocol::ParameterDomain;
use komorebi_protocol::ParameterId;
use komorebi_protocol::SelectorId;
use komorebi_protocol::WindowsPathInput;
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde::Serializer;
use serde::ser::Error as _;

use crate::ActionBindingError;

/// One scalar or scalar list before a catalog supplies its semantic domain.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActionInput(ActionInputKind);

impl ActionInput {
    #[must_use]
    pub fn scalar(value: ActionInputScalar) -> Self {
        Self(ActionInputKind::Scalar(value.0))
    }

    #[must_use]
    pub fn list(values: impl IntoIterator<Item = ActionInputScalar>) -> Self {
        Self(ActionInputKind::List(
            values
                .into_iter()
                .map(|value| value.0)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        ))
    }
}

impl From<ActionInputScalar> for ActionInput {
    fn from(value: ActionInputScalar) -> Self {
        Self::scalar(value)
    }
}

/// One non-list input value. Keeping this separate makes nested lists
/// unrepresentable for programmatic callers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActionInputScalar(InputScalar);

impl ActionInputScalar {
    #[must_use]
    pub const fn boolean(value: bool) -> Self {
        Self(InputScalar::Bool(value))
    }

    #[must_use]
    pub const fn signed(value: i64) -> Self {
        Self(InputScalar::Signed(value))
    }

    #[must_use]
    pub const fn unsigned(value: u64) -> Self {
        Self(InputScalar::Unsigned(value))
    }

    #[must_use]
    pub fn text(value: impl Into<Box<str>>) -> Self {
        Self(InputScalar::Text(value.into()))
    }

    pub fn windows_path_units(units: impl Into<Box<[u16]>>) -> Result<Self, ArgumentError> {
        Ok(Self(InputScalar::WindowsPath(WindowsPathInput::new(
            units,
        )?)))
    }

    /// Captures a native Windows path without Unicode repair.
    #[cfg(windows)]
    pub fn windows_path(path: &std::ffi::OsStr) -> Result<Self, ArgumentError> {
        use std::os::windows::ffi::OsStrExt as _;

        Self::windows_path_units(path.encode_wide().collect::<Vec<_>>())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ActionInputKind {
    Scalar(InputScalar),
    List(Box<[InputScalar]>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum InputScalar {
    Bool(bool),
    Signed(i64),
    Unsigned(u64),
    Text(Box<str>),
    WindowsPath(WindowsPathInput),
}

#[derive(Deserialize)]
#[serde(untagged)]
enum SerializedActionInput {
    Bool(bool),
    Signed(i64),
    Unsigned(u64),
    Text(Box<str>),
    List(Vec<SerializedActionScalar>),
}

#[derive(Deserialize)]
#[serde(untagged)]
enum SerializedActionScalar {
    Bool(bool),
    Signed(i64),
    Unsigned(u64),
    Text(Box<str>),
}

impl From<SerializedActionScalar> for InputScalar {
    fn from(value: SerializedActionScalar) -> Self {
        match value {
            SerializedActionScalar::Bool(value) => Self::Bool(value),
            SerializedActionScalar::Signed(value) => Self::Signed(value),
            SerializedActionScalar::Unsigned(value) => Self::Unsigned(value),
            SerializedActionScalar::Text(value) => Self::Text(value),
        }
    }
}

impl<'de> Deserialize<'de> for ActionInput {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(Self(
            match SerializedActionInput::deserialize(deserializer)? {
                SerializedActionInput::Bool(value) => {
                    ActionInputKind::Scalar(InputScalar::Bool(value))
                }
                SerializedActionInput::Signed(value) => {
                    ActionInputKind::Scalar(InputScalar::Signed(value))
                }
                SerializedActionInput::Unsigned(value) => {
                    ActionInputKind::Scalar(InputScalar::Unsigned(value))
                }
                SerializedActionInput::Text(value) => {
                    ActionInputKind::Scalar(InputScalar::Text(value))
                }
                SerializedActionInput::List(values) => ActionInputKind::List(
                    values
                        .into_iter()
                        .map(InputScalar::from)
                        .collect::<Vec<_>>()
                        .into_boxed_slice(),
                ),
            },
        ))
    }
}

impl Serialize for ActionInput {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match &self.0 {
            ActionInputKind::Scalar(value) => value.serialize(serializer),
            ActionInputKind::List(values) => values.serialize(serializer),
        }
    }
}

impl Serialize for InputScalar {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Bool(value) => serializer.serialize_bool(*value),
            Self::Signed(value) => serializer.serialize_i64(*value),
            Self::Unsigned(value) => serializer.serialize_u64(*value),
            Self::Text(value) => serializer.serialize_str(value),
            Self::WindowsPath(value) => {
                let text = String::from_utf16(value.units()).map_err(S::Error::custom)?;
                serializer.serialize_str(&text)
            }
        }
    }
}

pub(crate) fn bind_input(
    parameter: &ParameterId,
    domain: ParameterDomain,
    cardinality: ArgumentCardinality,
    input: &ActionInput,
) -> Result<ActionArgument, ActionBindingError> {
    match (cardinality, &input.0) {
        (
            ArgumentCardinality::RequiredScalar | ArgumentCardinality::OptionalScalar,
            ActionInputKind::Scalar(value),
        ) => Ok(ActionArgument::Scalar(bind_scalar(
            parameter, domain, value, false,
        )?)),
        (
            ArgumentCardinality::RequiredList | ArgumentCardinality::OptionalList,
            ActionInputKind::List(values),
        ) => Ok(ActionArgument::Scalars(ArgumentScalars::new(
            values
                .iter()
                .map(|value| bind_scalar(parameter, domain, value, true))
                .collect::<Result<Vec<_>, _>>()?,
        )?)),
        (
            ArgumentCardinality::RequiredScalar | ArgumentCardinality::OptionalScalar,
            ActionInputKind::List(_),
        ) => Err(ActionBindingError::ExpectedScalar(parameter.clone())),
        (
            ArgumentCardinality::RequiredList | ArgumentCardinality::OptionalList,
            ActionInputKind::Scalar(_),
        ) => Err(ActionBindingError::ExpectedList(parameter.clone())),
    }
}

fn bind_scalar(
    parameter: &ParameterId,
    domain: ParameterDomain,
    input: &InputScalar,
    list_item: bool,
) -> Result<ArgumentScalar, ActionBindingError> {
    use ParameterDomain as D;

    match domain {
        D::Flag => input_bool(parameter, domain, input).map(ArgumentScalar::Bool),
        D::Pixels
        | D::Adjustment
        | D::Size
        | D::ResizeStep
        | D::BorderWidth
        | D::BorderOffset
        | D::StackbarHeight
        | D::StackbarTabWidth
        | D::StackbarFontSize
        | D::WorkAreaOffset => input_signed(parameter, domain, input).map(ArgumentScalar::Signed),
        D::Index
        | D::Count
        | D::Columns
        | D::AtCount
        | D::Alpha
        | D::ColourChannel
        | D::AnimationDuration
        | D::AnimationFps => input_unsigned(parameter, domain, input).map(ArgumentScalar::Unsigned),
        D::Name | D::Executable | D::StackbarFontFamily => Ok(ArgumentScalar::Text(
            BoundedText::new(input_text(parameter, domain, input)?.to_owned())?,
        )),
        D::Path => match input {
            InputScalar::WindowsPath(value) => Ok(ArgumentScalar::WindowsPath(value.clone())),
            InputScalar::Text(value) => Ok(ArgumentScalar::WindowsPath(WindowsPathInput::new(
                value.encode_utf16().collect::<Vec<_>>(),
            )?)),
            _ => Err(domain_mismatch(parameter, domain)),
        },
        D::WorkspaceSelector | D::WindowSelector => Ok(ArgumentScalar::Selector(
            SelectorId::parse(input_text(parameter, domain, input)?.to_owned())?,
        )),
        D::Ratios => Ok(ArgumentScalar::Decimal(
            input_text(parameter, domain, input)?.parse()?,
        )),
        D::AnimationStyle if list_item => Ok(ArgumentScalar::Decimal(
            input_text(parameter, domain, input)?.parse()?,
        )),
        D::Direction
        | D::Axis
        | D::Layout
        | D::Cycle
        | D::Sizing
        | D::Behaviour
        | D::Implementation
        | D::Identifier
        | D::WindowKind
        | D::BorderStyle
        | D::BorderImplementation
        | D::StackbarMode
        | D::StackbarLabel
        | D::AnimationPrefix
        | D::AnimationStyle
        | D::CursorWarpPolicy
        | D::WorkspaceTarget => Ok(ArgumentScalar::Choice(ChoiceId::parse(
            input_text(parameter, domain, input)?.to_owned(),
        )?)),
    }
}

fn input_bool(
    parameter: &ParameterId,
    domain: ParameterDomain,
    input: &InputScalar,
) -> Result<bool, ActionBindingError> {
    match input {
        InputScalar::Bool(value) => Ok(*value),
        _ => Err(domain_mismatch(parameter, domain)),
    }
}

fn input_signed(
    parameter: &ParameterId,
    domain: ParameterDomain,
    input: &InputScalar,
) -> Result<i64, ActionBindingError> {
    match input {
        InputScalar::Signed(value) => Ok(*value),
        InputScalar::Unsigned(value) => {
            i64::try_from(*value).map_err(|_| domain_mismatch(parameter, domain))
        }
        _ => Err(domain_mismatch(parameter, domain)),
    }
}

fn input_unsigned(
    parameter: &ParameterId,
    domain: ParameterDomain,
    input: &InputScalar,
) -> Result<u64, ActionBindingError> {
    match input {
        InputScalar::Unsigned(value) => Ok(*value),
        InputScalar::Signed(value) => {
            u64::try_from(*value).map_err(|_| domain_mismatch(parameter, domain))
        }
        _ => Err(domain_mismatch(parameter, domain)),
    }
}

fn input_text<'a>(
    parameter: &ParameterId,
    domain: ParameterDomain,
    input: &'a InputScalar,
) -> Result<&'a str, ActionBindingError> {
    match input {
        InputScalar::Text(value) => Ok(value),
        _ => Err(domain_mismatch(parameter, domain)),
    }
}

pub(crate) fn domain_mismatch(
    parameter: &ParameterId,
    domain: ParameterDomain,
) -> ActionBindingError {
    ActionBindingError::InputDomainMismatch {
        parameter: parameter.clone(),
        domain,
    }
}
