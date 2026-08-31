//! Typed `LuaJIT` extension-host primitives.

mod domain;
mod plugin_vm;
mod sandbox;
#[cfg(windows)]
mod windows_sandbox;
#[cfg(windows)]
mod worker_probe;

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
pub use sandbox::SandboxIdentity;
#[cfg(windows)]
pub use windows_sandbox::LpacLaunchError;
#[cfg(windows)]
pub use windows_sandbox::LpacWorkerLauncher;
#[cfg(windows)]
pub use windows_sandbox::VerifiedLpacWorker;
#[cfg(windows)]
#[doc(hidden)]
pub use worker_probe::WorkerContainmentFailure;
#[cfg(windows)]
#[doc(hidden)]
pub use worker_probe::run_worker_containment_probe;
