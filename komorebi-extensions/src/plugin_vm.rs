mod action;
mod context;

use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use mlua::Function;
use mlua::HookTriggers;
use mlua::Lua;
use mlua::LuaOptions;
use mlua::StdLib;
use mlua::Table;
use mlua::Value;
use mlua::VmState;
use mlua::prelude::LuaChunkMode;
use thiserror::Error;

use crate::PluginCapability;
use crate::PluginLimits;
use crate::PluginManifest;
use crate::PluginProgram;

pub use self::action::PluginActionBuilder;
use self::context::HostCallFailure;
pub use self::context::PluginContext;
pub use self::context::PluginLogLevel;
pub use self::context::PluginLogRecord;
use crate::PluginOutputSink;

const HOOK_INTERVAL: u32 = 1_000;

/// Runtime profile selected before an extension starts and never changed in place.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PluginExecutionProfile {
    JitDisabled,
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
        outputs: impl PluginOutputSink,
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
            .create_userdata(PluginContext::new(plugin, capabilities, outputs))
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
    #[error("plugin log message exceeds the 16 KiB broker boundary")]
    LogMessageTooLarge,
    #[error("plugin lifecycle exceeded its structured output budget")]
    OutputBudgetExceeded,
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
                HostCallFailure::LogMessageTooLarge => Self::LogMessageTooLarge,
                HostCallFailure::OutputBudgetExceeded => Self::OutputBudgetExceeded,
            };
        }
        if matches!(error, mlua::Error::MemoryError(_)) {
            Self::MemoryBudgetExceeded
        } else {
            Self::Lua(error.to_string().into_boxed_str())
        }
    }
}
