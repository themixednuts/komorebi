//! Typed `LuaJIT` extension-host primitives.

mod domain;
mod plugin_vm;

pub use domain::InstructionBudget;
pub use domain::MemoryBudget;
pub use domain::PluginCapability;
pub use domain::PluginCapabilitySet;
pub use domain::PluginId;
pub use domain::PluginIdError;
pub use domain::PluginLimits;
pub use domain::PluginManifest;
pub use domain::PluginProgram;
pub use domain::PluginProgramError;
pub use plugin_vm::PluginContext;
pub use plugin_vm::PluginExecutionProfile;
pub use plugin_vm::PluginLogLevel;
pub use plugin_vm::PluginLogRecord;
pub use plugin_vm::PluginLogSink;
pub use plugin_vm::PluginVm;
pub use plugin_vm::PluginVmError;
