use std::collections::BTreeMap;

use komorebi_protocol::ActionArgument;
use komorebi_protocol::ActionArguments;
use komorebi_protocol::ActionAvailability;
use komorebi_protocol::ActionId;
use komorebi_protocol::ActionKey;
use komorebi_protocol::ArgumentCardinality;
use komorebi_protocol::ArgumentError;
use komorebi_protocol::ArgumentScalar;
use komorebi_protocol::ArgumentScalars;
use komorebi_protocol::BoundedText;
use komorebi_protocol::CatalogSnapshot;
use komorebi_protocol::ChoiceId;
use komorebi_protocol::ParameterDomain;
use komorebi_protocol::ParameterId;
use komorebi_protocol::SelectorId;
use komorebi_protocol::StableIdError;
use komorebi_protocol::WindowsPathInput;
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde::Serializer;
use serde::de::Error as _;
use serde::ser::Error as _;
use serde::ser::SerializeMap;
use serde::ser::SerializeStruct;
use thiserror::Error;

/// A renderer-independent action request as entered by a user-facing adapter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActionBinding {
    action: ActionId,
    arguments: BTreeMap<ParameterId, ActionInput>,
}

impl ActionBinding {
    #[must_use]
    pub const fn new(action: ActionId, arguments: BTreeMap<ParameterId, ActionInput>) -> Self {
        Self { action, arguments }
    }

    #[must_use]
    pub const fn action(&self) -> &ActionId {
        &self.action
    }

    /// Resolves this request against one immutable catalog snapshot.
    pub fn bind(&self, catalog: &CatalogSnapshot) -> Result<BoundAction, ActionBindingError> {
        let mut matches = catalog
            .definitions()
            .iter()
            .enumerate()
            .filter(|(_, definition)| definition.key().id() == &self.action);
        let Some((index, definition)) = matches.next() else {
            return Err(ActionBindingError::ActionNotOffered(self.action.clone()));
        };
        if matches.next().is_some() {
            return Err(ActionBindingError::AmbiguousAction(self.action.clone()));
        }
        let offer = catalog
            .offers()
            .get(index)
            .ok_or_else(|| ActionBindingError::ActionNotOffered(self.action.clone()))?;
        if let ActionAvailability::Unavailable(reason) = offer.availability() {
            return Err(ActionBindingError::Unavailable(reason));
        }

        for supplied in self.arguments.keys() {
            if !definition
                .parameters()
                .iter()
                .any(|parameter| parameter.id() == supplied)
            {
                return Err(ActionBindingError::UnknownParameter(supplied.clone()));
            }
        }

        let mut bound = BTreeMap::new();
        for parameter in definition.parameters() {
            let Some(input) = self.arguments.get(parameter.id()) else {
                if matches!(
                    parameter.cardinality(),
                    ArgumentCardinality::RequiredScalar | ArgumentCardinality::RequiredList
                ) {
                    return Err(ActionBindingError::MissingParameter(parameter.id().clone()));
                }
                continue;
            };
            let argument = bind_input(
                parameter.id(),
                parameter.domain(),
                parameter.cardinality(),
                input,
            )?;
            if let Some(choices) = offer
                .dynamic_choices()
                .iter()
                .find(|choices| choices.parameter() == parameter.id())
            {
                let supplied = match &argument {
                    ActionArgument::Scalar(value) => std::slice::from_ref(value),
                    ActionArgument::Scalars(values) => values.values(),
                };
                if supplied
                    .iter()
                    .any(|value| !choices.choices().contains(value))
                {
                    return Err(ActionBindingError::DynamicChoiceRejected(
                        parameter.id().clone(),
                    ));
                }
            }
            bound.insert(parameter.id().clone(), argument);
        }

        Ok(BoundAction {
            action: definition.key().clone(),
            arguments: ActionArguments::new(bound)?,
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SerializedActionBinding {
    action: String,
    #[serde(default)]
    arguments: BTreeMap<String, ActionInput>,
}

impl<'de> Deserialize<'de> for ActionBinding {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let serialized = SerializedActionBinding::deserialize(deserializer)?;
        let action = ActionId::parse(serialized.action).map_err(D::Error::custom)?;
        let arguments = serialized
            .arguments
            .into_iter()
            .map(|(id, value)| {
                ParameterId::parse(id)
                    .map(|id| (id, value))
                    .map_err(D::Error::custom)
            })
            .collect::<Result<_, _>>()?;
        Ok(Self::new(action, arguments))
    }
}

impl Serialize for ActionBinding {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("ActionBinding", 2)?;
        state.serialize_field("action", self.action.as_str())?;
        state.serialize_field("arguments", &SerializedArguments(&self.arguments))?;
        state.end()
    }
}

struct SerializedArguments<'a>(&'a BTreeMap<ParameterId, ActionInput>);

impl Serialize for SerializedArguments<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (id, input) in self.0 {
            map.serialize_entry(id.as_str(), input)?;
        }
        map.end()
    }
}

#[cfg(feature = "schemars")]
impl schemars::JsonSchema for ActionBinding {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("ActionBinding")
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "object",
            "additionalProperties": false,
            "required": ["action"],
            "properties": {
                "action": {
                    "type": "string",
                    "pattern": "^[a-z0-9.-]+$",
                    "description": "Stable action ID from the current manager catalog"
                },
                "arguments": {
                    "type": "object",
                    "description": "Named values resolved against the action's current catalog schema",
                    "additionalProperties": {
                        "oneOf": [
                            { "type": "boolean" },
                            { "type": "integer" },
                            { "type": "string" },
                            {
                                "type": "array",
                                "items": {
                                    "oneOf": [
                                        { "type": "boolean" },
                                        { "type": "integer" },
                                        { "type": "string" }
                                    ]
                                }
                            }
                        ]
                    }
                }
            }
        })
    }
}

/// A catalog-bound action ready for exact command submission.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundAction {
    action: ActionKey,
    arguments: ActionArguments,
}

impl BoundAction {
    #[must_use]
    pub const fn action(&self) -> &ActionKey {
        &self.action
    }

    #[must_use]
    pub const fn arguments(&self) -> &ActionArguments {
        &self.arguments
    }

    #[must_use]
    pub fn into_parts(self) -> (ActionKey, ActionArguments) {
        (self.action, self.arguments)
    }
}

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

fn bind_input(
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

fn domain_mismatch(parameter: &ParameterId, domain: ParameterDomain) -> ActionBindingError {
    ActionBindingError::InputDomainMismatch {
        parameter: parameter.clone(),
        domain,
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ActionBindingError {
    #[error("the current catalog does not offer action {0}")]
    ActionNotOffered(ActionId),
    #[error("the current catalog offers multiple schemas for action {0}")]
    AmbiguousAction(ActionId),
    #[error("the action is currently unavailable: {0:?}")]
    Unavailable(komorebi_protocol::ActionUnavailability),
    #[error("the binding supplied unknown parameter {0:?}")]
    UnknownParameter(ParameterId),
    #[error("the binding omitted required parameter {0:?}")]
    MissingParameter(ParameterId),
    #[error("parameter {0:?} requires one scalar value")]
    ExpectedScalar(ParameterId),
    #[error("parameter {0:?} requires a scalar list")]
    ExpectedList(ParameterId),
    #[error("parameter {parameter:?} does not accept this value for catalog domain {domain:?}")]
    InputDomainMismatch {
        parameter: ParameterId,
        domain: ParameterDomain,
    },
    #[error("a value is not one of the current choices for parameter {0:?}")]
    DynamicChoiceRejected(ParameterId),
    #[error(transparent)]
    StableId(#[from] StableIdError),
    #[error(transparent)]
    Argument(#[from] ArgumentError),
}
