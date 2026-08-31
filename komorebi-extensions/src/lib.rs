//! Typed `LuaJIT` extension-host primitives.

mod domain;
mod host_domain;
#[cfg(windows)]
mod hot_reload;
#[cfg(windows)]
mod plugin_host;
mod plugin_vm;
mod sandbox;
#[cfg(windows)]
mod windows_sandbox;
mod wire;
#[cfg(windows)]
mod worker_probe;
#[cfg(windows)]
mod worker_runtime;

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
pub use host_domain::PluginActionRequest;
pub use host_domain::PluginLoadFailure;
pub use host_domain::PluginLoadReport;
pub use host_domain::PluginOutput;
pub use host_domain::PluginOutputSink;
#[cfg(windows)]
pub use hot_reload::PluginHotReloadEvent;
#[cfg(windows)]
pub use hot_reload::PluginHotReloadEventCapacity;
#[cfg(windows)]
pub use hot_reload::PluginHotReloadQuietPeriod;
#[cfg(windows)]
pub use hot_reload::PluginHotReloadService;
#[cfg(windows)]
pub use hot_reload::PluginHotReloadSettings;
#[cfg(windows)]
pub use hot_reload::PluginHotReloadShutdownError;
#[cfg(windows)]
pub use hot_reload::PluginHotReloadStartError;
#[cfg(windows)]
pub use hot_reload::PluginSourceFile;
#[cfg(windows)]
pub use hot_reload::PluginSourceLoadError;
#[cfg(windows)]
pub use hot_reload::PluginSourceOpenError;
#[cfg(windows)]
pub use hot_reload::PluginWatchFailure;
#[cfg(windows)]
pub use plugin_host::PluginHostClient;
#[cfg(windows)]
pub use plugin_host::PluginHostQueueCapacity;
#[cfg(windows)]
pub use plugin_host::PluginHostService;
#[cfg(windows)]
pub use plugin_host::PluginHostShutdownError;
#[cfg(windows)]
pub use plugin_host::PluginHostStartError;
#[cfg(windows)]
pub use plugin_host::PluginReloadError;
pub use plugin_vm::PluginActionBuilder;
pub use plugin_vm::PluginContext;
pub use plugin_vm::PluginExecutionProfile;
pub use plugin_vm::PluginLogLevel;
pub use plugin_vm::PluginLogRecord;
pub use plugin_vm::PluginVm;
pub use plugin_vm::PluginVmError;
pub use sandbox::SandboxIdentity;
#[cfg(windows)]
pub use windows_sandbox::LpacLaunchError;
#[cfg(windows)]
pub use windows_sandbox::LpacSessionError;
#[cfg(windows)]
pub use windows_sandbox::LpacWorkerLauncher;
#[cfg(windows)]
pub use windows_sandbox::VerifiedLpacWorker;
#[doc(hidden)]
pub use wire::WireError;
#[cfg(windows)]
#[doc(hidden)]
pub use worker_probe::WorkerContainmentFailure;
#[cfg(windows)]
#[doc(hidden)]
pub use worker_probe::run_worker_containment_probe;
#[cfg(windows)]
#[doc(hidden)]
pub use worker_runtime::run_extension_worker;
