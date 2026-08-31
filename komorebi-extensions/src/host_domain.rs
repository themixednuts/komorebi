use thiserror::Error;

use crate::PluginCapability;
use crate::PluginLogRecord;
use crate::PluginVmError;

pub(crate) const MAX_PLUGIN_LOG_RECORDS: usize = 64;
pub(crate) const MAX_PLUGIN_LOG_MESSAGE_BYTES: usize = 16 * 1024;

/// Successful replacement of the worker's active plugin VM.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PluginLoadReport {
    logs: Box<[PluginLogRecord]>,
}

impl PluginLoadReport {
    pub(crate) fn new(logs: Vec<PluginLogRecord>) -> Self {
        Self {
            logs: logs.into_boxed_slice(),
        }
    }

    #[must_use]
    pub fn logs(&self) -> &[PluginLogRecord] {
        &self.logs
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
    #[error("plugin lifecycle exceeded its structured log budget")]
    LogBudgetExceeded,
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
            PluginVmError::LogBudgetExceeded => Self::LogBudgetExceeded,
            PluginVmError::Lua(message) => Self::Lua(message),
        }
    }
}
