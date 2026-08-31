use komorebi_protocol::ActionIntent;
use thiserror::Error;

use crate::PluginCapability;
use crate::PluginId;
use crate::PluginLogRecord;
use crate::PluginVmError;

pub(crate) const MAX_PLUGIN_OUTPUTS: usize = 64;
pub(crate) const MAX_PLUGIN_LOG_MESSAGE_BYTES: usize = 16 * 1024;

/// An authorized manager action requested by one identified extension.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PluginActionRequest {
    plugin: PluginId,
    intent: ActionIntent,
}

impl PluginActionRequest {
    pub(crate) const fn new(plugin: PluginId, intent: ActionIntent) -> Self {
        Self { plugin, intent }
    }

    #[must_use]
    pub const fn plugin(&self) -> &PluginId {
        &self.plugin
    }

    #[must_use]
    pub const fn intent(&self) -> &ActionIntent {
        &self.intent
    }
}

/// Ordered, bounded output from one extension lifecycle callback.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PluginOutput {
    Log(PluginLogRecord),
    InvokeAction(PluginActionRequest),
}

/// Consumer-owned port that records callback output without granting authority.
pub trait PluginOutputSink: Send + Sync + 'static {
    fn emit(&self, output: PluginOutput);
}

/// Successful replacement of the worker's active plugin VM.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PluginLoadReport {
    outputs: Box<[PluginOutput]>,
}

impl PluginLoadReport {
    pub(crate) fn new(outputs: Vec<PluginOutput>) -> Self {
        Self {
            outputs: outputs.into_boxed_slice(),
        }
    }

    #[must_use]
    pub fn outputs(&self) -> &[PluginOutput] {
        &self.outputs
    }
}

/// Typed failure returned by the isolated VM without exposing process authority.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum PluginLoadFailure {
    #[error("plugin capability denied: {0:?}")]
    CapabilityDenied(PluginCapability),
    #[error("plugin instruction budget exhausted")]
    InstructionBudgetExhausted,
    #[error("plugin VM memory budget was exceeded")]
    MemoryBudgetExceeded,
    #[error("plugin VM memory limit overflowed")]
    MemoryLimitOverflow,
    #[error("this LuaJIT allocator cannot enforce a memory limit")]
    MemoryLimitUnavailable,
    #[error("plugin module must return a table containing on_load(context)")]
    MissingOnLoad,
    #[error("plugin log message exceeds the 16 KiB broker boundary")]
    LogMessageTooLarge,
    #[error("plugin lifecycle exceeded its structured output budget")]
    OutputBudgetExceeded,
    #[error("LuaJIT rejected the plugin: {0}")]
    Lua(Box<str>),
}

impl From<PluginVmError> for PluginLoadFailure {
    fn from(error: PluginVmError) -> Self {
        match error {
            PluginVmError::CapabilityDenied(capability) => Self::CapabilityDenied(capability),
            PluginVmError::InstructionBudgetExhausted => Self::InstructionBudgetExhausted,
            PluginVmError::MemoryBudgetExceeded => Self::MemoryBudgetExceeded,
            PluginVmError::MemoryLimitOverflow => Self::MemoryLimitOverflow,
            PluginVmError::MemoryLimitUnavailable => Self::MemoryLimitUnavailable,
            PluginVmError::MissingOnLoad => Self::MissingOnLoad,
            PluginVmError::LogMessageTooLarge => Self::LogMessageTooLarge,
            PluginVmError::OutputBudgetExceeded => Self::OutputBudgetExceeded,
            PluginVmError::Lua(message) => Self::Lua(message),
        }
    }
}
