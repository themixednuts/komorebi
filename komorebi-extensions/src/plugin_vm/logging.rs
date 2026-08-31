use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use mlua::UserData;
use mlua::UserDataFields;
use mlua::UserDataMethods;
use thiserror::Error;

use crate::PluginCapability;
use crate::PluginCapabilitySet;
use crate::PluginId;
use crate::host_domain::MAX_PLUGIN_LOG_MESSAGE_BYTES;
use crate::host_domain::MAX_PLUGIN_LOG_RECORDS;

/// Structured severity accepted by the first brokered extension capability.
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

pub trait PluginLogSink: Send + Sync + 'static {
    fn emit(&self, record: PluginLogRecord);
}

/// Capability-checked object passed to every extension lifecycle callback.
pub struct PluginContext {
    plugin: PluginId,
    capabilities: PluginCapabilitySet,
    logs: Arc<dyn PluginLogSink>,
    remaining_logs: AtomicUsize,
}

impl PluginContext {
    pub(super) fn new(
        plugin: PluginId,
        capabilities: PluginCapabilitySet,
        logs: impl PluginLogSink,
    ) -> Self {
        Self {
            plugin,
            capabilities,
            logs: Arc::new(logs),
            remaining_logs: AtomicUsize::new(MAX_PLUGIN_LOG_RECORDS),
        }
    }
}

macro_rules! add_log_method {
    ($methods:ident, $name:literal, $level:expr) => {
        $methods.add_method($name, |_, context, message: String| {
            if !context.capabilities.allows(PluginCapability::Log) {
                return Err(mlua::Error::external(HostCallFailure::CapabilityDenied(
                    PluginCapability::Log,
                )));
            }
            if message.len() > MAX_PLUGIN_LOG_MESSAGE_BYTES {
                return Err(mlua::Error::external(HostCallFailure::LogMessageTooLarge));
            }
            if context
                .remaining_logs
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |remaining| {
                    remaining.checked_sub(1)
                })
                .is_err()
            {
                return Err(mlua::Error::external(HostCallFailure::LogBudgetExceeded));
            }
            context.logs.emit(PluginLogRecord {
                plugin: context.plugin.clone(),
                level: $level,
                message: message.into_boxed_str(),
            });
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
    #[error("plugin lifecycle exceeded its structured log budget")]
    LogBudgetExceeded,
}
