use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use komorebi_protocol::ArgumentScalar;
use komorebi_protocol::BoundedText;
use komorebi_protocol::ChoiceId;
use komorebi_protocol::Color;
use komorebi_protocol::EntityId;
use komorebi_protocol::EntityKind;
use komorebi_protocol::EntityReference;
use komorebi_protocol::FixedDecimal;
use komorebi_protocol::SelectorId;
use komorebi_protocol::Unit;
use komorebi_protocol::UnitValue;
use komorebi_protocol::WindowsPathInput;
use mlua::UserData;
use mlua::UserDataFields;
use mlua::UserDataMethods;
use thiserror::Error;

use crate::PluginActionRequest;
use crate::PluginCapability;
use crate::PluginCapabilitySet;
use crate::PluginId;
use crate::PluginOutput;
use crate::PluginOutputSink;
use crate::host_domain::MAX_PLUGIN_LOG_MESSAGE_BYTES;
use crate::host_domain::MAX_PLUGIN_OUTPUTS;

use super::action::PluginActionBuilder;
use super::action::PluginActionInputError;
use super::action::PluginValue;

/// Structured severity accepted by the brokered extension API.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PluginLogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl PluginLogLevel {
    pub(crate) const fn code(self) -> u8 {
        self as u8
    }

    pub(crate) const fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Self::Trace),
            1 => Some(Self::Debug),
            2 => Some(Self::Info),
            3 => Some(Self::Warn),
            4 => Some(Self::Error),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PluginLogRecord {
    plugin: PluginId,
    level: PluginLogLevel,
    message: Box<str>,
}

impl PluginLogRecord {
    pub(crate) fn new(plugin: PluginId, level: PluginLogLevel, message: Box<str>) -> Self {
        Self {
            plugin,
            level,
            message,
        }
    }

    #[must_use]
    pub const fn plugin(&self) -> &PluginId {
        &self.plugin
    }

    #[must_use]
    pub const fn level(&self) -> PluginLogLevel {
        self.level
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

/// Capability-checked object passed to every extension lifecycle callback.
pub struct PluginContext {
    plugin: PluginId,
    capabilities: PluginCapabilitySet,
    outputs: Arc<dyn PluginOutputSink>,
    remaining_outputs: AtomicUsize,
}

impl PluginContext {
    pub(super) fn new(
        plugin: PluginId,
        capabilities: PluginCapabilitySet,
        outputs: impl PluginOutputSink,
    ) -> Self {
        Self {
            plugin,
            capabilities,
            outputs: Arc::new(outputs),
            remaining_outputs: AtomicUsize::new(MAX_PLUGIN_OUTPUTS),
        }
    }

    pub(super) fn require(&self, capability: PluginCapability) -> mlua::Result<()> {
        if self.capabilities.allows(capability) {
            Ok(())
        } else {
            Err(mlua::Error::external(HostCallFailure::CapabilityDenied(
                capability,
            )))
        }
    }

    pub(super) const fn plugin(&self) -> &PluginId {
        &self.plugin
    }

    pub(super) fn emit(&self, output: PluginOutput) -> mlua::Result<()> {
        if self
            .remaining_outputs
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |remaining| {
                remaining.checked_sub(1)
            })
            .is_err()
        {
            return Err(mlua::Error::external(HostCallFailure::OutputBudgetExceeded));
        }
        self.outputs.emit(output);
        Ok(())
    }
}

macro_rules! add_log_method {
    ($methods:ident, $name:literal, $level:expr) => {
        $methods.add_method($name, |_, context, message: String| {
            context.require(PluginCapability::Log)?;
            if message.len() > MAX_PLUGIN_LOG_MESSAGE_BYTES {
                return Err(mlua::Error::external(HostCallFailure::LogMessageTooLarge));
            }
            context.emit(PluginOutput::Log(PluginLogRecord::new(
                context.plugin.clone(),
                $level,
                message.into_boxed_str(),
            )))?;
            Ok(())
        });
    };
}

impl UserData for PluginContext {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("plugin_id", |_, context| {
            Ok(context.plugin.as_str().to_owned())
        });
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        add_log_method!(methods, "trace", PluginLogLevel::Trace);
        add_log_method!(methods, "debug", PluginLogLevel::Debug);
        add_log_method!(methods, "info", PluginLogLevel::Info);
        add_log_method!(methods, "warn", PluginLogLevel::Warn);
        add_log_method!(methods, "error", PluginLogLevel::Error);
        methods.add_method("action", |_, _, action: String| {
            PluginActionBuilder::new(action).map_err(mlua::Error::external)
        });
        methods.add_method(
            "invoke",
            |_, context, action: PluginActionBuilder| -> mlua::Result<()> {
                context.require(PluginCapability::InvokeAction)?;
                let request = PluginActionRequest::new(
                    context.plugin().clone(),
                    action.finish().map_err(mlua::Error::external)?,
                );
                context.emit(PluginOutput::InvokeAction(request))?;
                Ok(())
            },
        );
        methods.add_method("boolean", |_, _, value: bool| {
            Ok(PluginValue::new(ArgumentScalar::Bool(value)))
        });
        methods.add_method("signed", |_, _, value: i64| {
            Ok(PluginValue::new(ArgumentScalar::Signed(value)))
        });
        methods.add_method("unsigned", |_, _, value: u64| {
            Ok(PluginValue::new(ArgumentScalar::Unsigned(value)))
        });
        methods.add_method("decimal", |_, _, value: String| {
            let value = FixedDecimal::from_str(&value).map_err(action_error)?;
            Ok(PluginValue::new(ArgumentScalar::Decimal(value)))
        });
        methods.add_method("text", |_, _, value: String| {
            let value = BoundedText::new(value).map_err(action_error)?;
            Ok(PluginValue::new(ArgumentScalar::Text(value)))
        });
        methods.add_method("choice", |_, _, value: String| {
            let value = ChoiceId::parse(value).map_err(action_error)?;
            Ok(PluginValue::new(ArgumentScalar::Choice(value)))
        });
        methods.add_method(
            "color",
            |_, _, (red, green, blue, alpha): (u16, u16, u16, u16)| {
                Ok(PluginValue::new(ArgumentScalar::Color(Color::new(
                    red, green, blue, alpha,
                ))))
            },
        );
        methods.add_method("unit", |_, _, (unit, magnitude): (String, i64)| {
            let unit = parse_unit(&unit)?;
            Ok(PluginValue::new(ArgumentScalar::Unit(UnitValue::new(
                unit, magnitude,
            ))))
        });
        methods.add_method("entity", |_, _, (kind, id): (String, String)| {
            let value = EntityReference::new(
                EntityKind::parse(kind).map_err(action_error)?,
                EntityId::parse(id).map_err(action_error)?,
            );
            Ok(PluginValue::new(ArgumentScalar::Entity(value)))
        });
        methods.add_method("selector", |_, _, value: String| {
            let value = SelectorId::parse(value).map_err(action_error)?;
            Ok(PluginValue::new(ArgumentScalar::Selector(value)))
        });
        methods.add_method("windows_path", |_, _, units: Vec<u16>| {
            let value = WindowsPathInput::new(units.into_boxed_slice()).map_err(action_error)?;
            Ok(PluginValue::new(ArgumentScalar::WindowsPath(value)))
        });
    }
}

fn action_error(error: impl Into<PluginActionInputError>) -> mlua::Error {
    mlua::Error::external(error.into())
}

fn parse_unit(value: &str) -> mlua::Result<Unit> {
    match value {
        "pixels" => Ok(Unit::Pixels),
        "basis-points" => Ok(Unit::BasisPoints),
        "milliseconds" => Ok(Unit::Milliseconds),
        _ => Err(mlua::Error::runtime(format!("unknown action unit {value}"))),
    }
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub(super) enum HostCallFailure {
    #[error("plugin capability denied: {0:?}")]
    CapabilityDenied(PluginCapability),
    #[error("plugin instruction budget exhausted")]
    InstructionBudgetExhausted,
    #[error("plugin log message exceeds the broker boundary")]
    LogMessageTooLarge,
    #[error("plugin lifecycle exceeded its structured output budget")]
    OutputBudgetExceeded,
}
