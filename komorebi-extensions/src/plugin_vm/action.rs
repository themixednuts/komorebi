use komorebi_protocol::ActionArgument;
use komorebi_protocol::ActionArguments;
use komorebi_protocol::ActionId;
use komorebi_protocol::ActionIntent;
use komorebi_protocol::ArgumentScalar;
use komorebi_protocol::ArgumentScalars;
use komorebi_protocol::ParameterId;
use mlua::FromLua;
use mlua::Lua;
use mlua::UserData;
use mlua::UserDataMethods;
use mlua::Value;
use std::collections::BTreeMap;
use thiserror::Error;

/// Opaque, typed protocol scalar used to construct an action request in Lua.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PluginValue(pub(super) ArgumentScalar);

impl PluginValue {
    pub(super) const fn new(value: ArgumentScalar) -> Self {
        Self(value)
    }
}

impl UserData for PluginValue {}

impl FromLua for PluginValue {
    fn from_lua(value: Value, _lua: &Lua) -> mlua::Result<Self> {
        let Value::UserData(value) = value else {
            return Err(mlua::Error::FromLuaConversionError {
                from: value.type_name(),
                to: "PluginValue".to_owned(),
                message: Some("expected a value created by PluginContext".to_owned()),
            });
        };
        let scalar = value.borrow::<Self>()?.clone();
        Ok(scalar)
    }
}

/// Mutable Lua-side request builder that becomes immutable at invocation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PluginActionBuilder {
    action: ActionId,
    arguments: BTreeMap<ParameterId, ActionArgument>,
}

impl PluginActionBuilder {
    pub(super) fn new(action: String) -> Result<Self, PluginActionInputError> {
        Ok(Self {
            action: ActionId::parse(action)?,
            arguments: BTreeMap::new(),
        })
    }

    fn insert(
        &mut self,
        parameter: String,
        argument: ActionArgument,
    ) -> Result<(), PluginActionInputError> {
        let parameter = ParameterId::parse(parameter)?;
        if self.arguments.insert(parameter.clone(), argument).is_some() {
            return Err(PluginActionInputError::DuplicateParameter(parameter));
        }
        Ok(())
    }

    pub(super) fn finish(self) -> Result<ActionIntent, PluginActionInputError> {
        Ok(ActionIntent::new(
            self.action,
            ActionArguments::new(self.arguments)?,
        ))
    }
}

impl UserData for PluginActionBuilder {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method_mut(
            "set",
            |_, action, (parameter, value): (String, PluginValue)| -> mlua::Result<()> {
                action
                    .insert(parameter, ActionArgument::Scalar(value.0))
                    .map_err(mlua::Error::external)?;
                Ok(())
            },
        );
        methods.add_method_mut(
            "set_list",
            |_, action, (parameter, values): (String, Vec<PluginValue>)| -> mlua::Result<()> {
                let values = values.into_iter().map(|value| value.0).collect::<Vec<_>>();
                let values =
                    ArgumentScalars::new(values.into_boxed_slice()).map_err(action_error)?;
                action
                    .insert(parameter, ActionArgument::Scalars(values))
                    .map_err(mlua::Error::external)?;
                Ok(())
            },
        );
    }
}

impl FromLua for PluginActionBuilder {
    fn from_lua(value: Value, _lua: &Lua) -> mlua::Result<Self> {
        let Value::UserData(value) = value else {
            return Err(mlua::Error::FromLuaConversionError {
                from: value.type_name(),
                to: "PluginActionBuilder".to_owned(),
                message: Some("expected an action created by PluginContext".to_owned()),
            });
        };
        let action = value.borrow::<Self>()?.clone();
        Ok(action)
    }
}

fn action_error(error: impl Into<PluginActionInputError>) -> mlua::Error {
    mlua::Error::external(error.into())
}

#[derive(Debug, Error)]
pub(super) enum PluginActionInputError {
    #[error("action parameter {0:?} was supplied more than once")]
    DuplicateParameter(ParameterId),
    #[error(transparent)]
    StableId(#[from] komorebi_protocol::StableIdError),
    #[error(transparent)]
    Argument(#[from] komorebi_protocol::ArgumentError),
}
