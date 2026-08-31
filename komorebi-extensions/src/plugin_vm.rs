use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use mlua::Function;
use mlua::HookTriggers;
use mlua::Lua;
use mlua::LuaOptions;
use mlua::StdLib;
use mlua::Table;
use mlua::UserData;
use mlua::UserDataFields;
use mlua::UserDataMethods;
use mlua::Value;
use mlua::VmState;
use mlua::prelude::LuaChunkMode;
use thiserror::Error;

use crate::PluginCapability;
use crate::PluginCapabilitySet;
use crate::PluginId;
use crate::PluginLimits;
use crate::PluginManifest;
use crate::PluginProgram;

const HOOK_INTERVAL: u32 = 1_000;

/// Runtime profile selected before an extension starts and never changed in place.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PluginExecutionProfile {
    JitDisabled,
}

/// Structured severity accepted by the first brokered extension capability.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PluginLogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PluginLogRecord {
    plugin: PluginId,
    level: PluginLogLevel,
    message: Box<str>,
}

impl PluginLogRecord {
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
}

macro_rules! add_log_method {
    ($methods:ident, $name:literal, $level:expr) => {
        $methods.add_method($name, |_, context, message: String| {
            if !context.capabilities.allows(PluginCapability::Log) {
                return Err(mlua::Error::external(HostCallFailure::CapabilityDenied(
                    PluginCapability::Log,
                )));
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

/// One `LuaJIT` VM owned exclusively by one extension.
pub struct PluginVm {
    lua: Lua,
    environment: Table,
    context: mlua::AnyUserData,
    instruction_budget: u64,
    remaining_instructions: Arc<AtomicU64>,
}

impl PluginVm {
    pub fn new(
        manifest: PluginManifest,
        limits: PluginLimits,
        logs: impl PluginLogSink,
    ) -> Result<Self, PluginVmError> {
        let (plugin, capabilities) = manifest.into_parts();
        let libraries = StdLib::TABLE | StdLib::STRING | StdLib::MATH | StdLib::BIT | StdLib::JIT;
        let lua = Lua::new_with(libraries, LuaOptions::new().catch_rust_panics(false))
            .map_err(|error| PluginVmError::from_lua(&error))?;
        lua.load("jit.off(); jit.flush()")
            .set_name("=komorebi-jit-policy")
            .exec()
            .map_err(|error| PluginVmError::from_lua(&error))?;

        let environment =
            safe_environment(&lua).map_err(|error| PluginVmError::from_lua(&error))?;
        let context = lua
            .create_userdata(PluginContext {
                plugin,
                capabilities,
                logs: Arc::new(logs),
            })
            .map_err(|error| PluginVmError::from_lua(&error))?;

        let absolute_memory_limit = lua
            .used_memory()
            .checked_add(limits.memory().bytes())
            .ok_or(PluginVmError::MemoryLimitOverflow)?;
        lua.set_memory_limit(absolute_memory_limit)
            .map_err(|_| PluginVmError::MemoryLimitUnavailable)?;

        let instruction_budget = limits.instructions().instructions();
        let remaining_instructions = Arc::new(AtomicU64::new(instruction_budget));
        let hook_counter = Arc::clone(&remaining_instructions);
        lua.set_hook(
            HookTriggers::new().every_nth_instruction(HOOK_INTERVAL),
            move |_, _| {
                let consumed = u64::from(HOOK_INTERVAL);
                if hook_counter
                    .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |remaining| {
                        (remaining > consumed).then(|| remaining - consumed)
                    })
                    .is_err()
                {
                    return Err(mlua::Error::external(
                        HostCallFailure::InstructionBudgetExhausted,
                    ));
                }
                Ok(VmState::Continue)
            },
        )
        .map_err(|error| PluginVmError::from_lua(&error))?;

        Ok(Self {
            lua,
            environment,
            context,
            instruction_budget,
            remaining_instructions,
        })
    }

    #[must_use]
    pub const fn profile(&self) -> PluginExecutionProfile {
        PluginExecutionProfile::JitDisabled
    }

    pub fn load(&self, program: PluginProgram) -> Result<(), PluginVmError> {
        let (name, source) = program.into_parts();
        self.reset_instruction_budget();
        let module: Table = self
            .lua
            .load(source.as_ref())
            .set_mode(LuaChunkMode::Text)
            .set_name(name.as_ref())
            .set_environment(self.environment.clone())
            .eval()
            .map_err(|error| PluginVmError::from_lua(&error))?;
        let on_load: Function = module
            .get("on_load")
            .map_err(|_| PluginVmError::MissingOnLoad)?;
        self.reset_instruction_budget();
        on_load
            .call::<()>(self.context.clone())
            .map_err(|error| PluginVmError::from_lua(&error))
    }

    fn reset_instruction_budget(&self) {
        self.remaining_instructions
            .store(self.instruction_budget, Ordering::Relaxed);
    }
}

fn safe_environment(lua: &Lua) -> mlua::Result<Table> {
    let globals = lua.globals();
    let environment = lua.create_table()?;
    for name in [
        "assert", "error", "ipairs", "next", "pairs", "pcall", "select", "tonumber", "tostring",
        "type", "xpcall", "math", "string", "table", "bit",
    ] {
        environment.set(name, globals.get::<Value>(name)?)?;
    }
    Ok(environment)
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
enum HostCallFailure {
    #[error("plugin capability denied: {0:?}")]
    CapabilityDenied(PluginCapability),
    #[error("plugin instruction budget exhausted")]
    InstructionBudgetExhausted,
}

#[derive(Debug, Error)]
pub enum PluginVmError {
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
    #[error("LuaJIT rejected the plugin: {0}")]
    Lua(Box<str>),
}

impl PluginVmError {
    fn from_lua(error: &mlua::Error) -> Self {
        if let Some(host) = error.downcast_ref::<HostCallFailure>() {
            return match host {
                HostCallFailure::CapabilityDenied(capability) => {
                    Self::CapabilityDenied(*capability)
                }
                HostCallFailure::InstructionBudgetExhausted => Self::InstructionBudgetExhausted,
            };
        }
        if matches!(error, mlua::Error::MemoryError(_)) {
            Self::MemoryBudgetExceeded
        } else {
            Self::Lua(error.to_string().into_boxed_str())
        }
    }
}
