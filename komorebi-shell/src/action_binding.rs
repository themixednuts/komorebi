use std::collections::BTreeMap;

use komorebi_protocol::ActionArguments;
use komorebi_protocol::ActionId;
use komorebi_protocol::ArgumentCardinality;
use komorebi_protocol::ArgumentError;
use komorebi_protocol::CatalogSnapshot;
use komorebi_protocol::ParameterDomain;
use komorebi_protocol::ParameterId;
use komorebi_protocol::StableIdError;
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde::Serializer;
use serde::de::Error as _;
use serde::ser::SerializeMap;
use serde::ser::SerializeStruct;
use thiserror::Error;

use crate::ActionInput;
use crate::BoundAction;
use crate::action_input::bind_input;
use crate::bound_action::offered_action;
use crate::bound_action::validate_dynamic_choices;

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
        let (definition, offer) = offered_action(catalog, &self.action)?;

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
            validate_dynamic_choices(offer, parameter.id(), &argument)?;
            bound.insert(parameter.id().clone(), argument);
        }

        Ok(BoundAction::new(
            definition.key().clone(),
            ActionArguments::new(bound)?,
        ))
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
