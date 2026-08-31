use std::sync::Arc;
use std::sync::Mutex;

use komorebi_extensions::InstructionBudget;
use komorebi_extensions::MemoryBudget;
use komorebi_extensions::PluginCapability;
use komorebi_extensions::PluginCapabilitySet;
use komorebi_extensions::PluginId;
use komorebi_extensions::PluginLimits;
use komorebi_extensions::PluginLogLevel;
use komorebi_extensions::PluginLogRecord;
use komorebi_extensions::PluginLogSink;
use komorebi_extensions::PluginManifest;
use komorebi_extensions::PluginProgram;
use komorebi_extensions::PluginProgramError;
use komorebi_extensions::PluginVm;
use komorebi_extensions::PluginVmError;

#[derive(Clone, Default)]
struct RecordingLogs(Arc<Mutex<Vec<PluginLogRecord>>>);

impl PluginLogSink for RecordingLogs {
    fn emit(&self, record: PluginLogRecord) {
        if let Ok(mut records) = self.0.lock() {
            records.push(record);
        }
    }
}

fn manifest(
    capabilities: PluginCapabilitySet,
) -> Result<PluginManifest, Box<dyn std::error::Error>> {
    Ok(PluginManifest::new(
        PluginId::parse("workspace-labels")?,
        capabilities,
    ))
}

fn limits() -> Result<PluginLimits, Box<dyn std::error::Error>> {
    Ok(PluginLimits::new(
        MemoryBudget::new(2 * 1024 * 1024).ok_or("memory budget must be nonzero")?,
        InstructionBudget::new(100_000).ok_or("instruction budget must be nonzero")?,
    ))
}

#[test]
fn plugin_program_rejects_binary_lua_before_it_reaches_the_vm() {
    let result = PluginProgram::new("bytecode.lua", [0x1b, b'L', b'J', 2, 0xff]);

    assert_eq!(result, Err(PluginProgramError::SourceNotUtf8));
}

#[test]
fn plugin_program_rejects_source_larger_than_the_broker_frame_budget() {
    let oversized = vec![b' '; 1024 * 1024 + 1];

    assert_eq!(
        PluginProgram::new("oversized.lua", oversized),
        Err(PluginProgramError::SourceTooLarge)
    );
}

#[test]
fn plugin_logging_is_bounded_before_reaching_the_sink() -> Result<(), Box<dyn std::error::Error>> {
    let logs = RecordingLogs::default();
    let vm = PluginVm::new(
        manifest(PluginCapabilitySet::only([PluginCapability::Log]))?,
        limits()?,
        logs.clone(),
    )?;
    let oversized_message = "x".repeat(16 * 1024 + 1);
    let source =
        format!("return {{ on_load = function(context) context:info('{oversized_message}') end }}");

    assert!(matches!(
        vm.load(PluginProgram::new("oversized-log.lua", source)?),
        Err(PluginVmError::LogMessageTooLarge)
    ));
    assert!(logs.0.lock().map_err(|error| error.to_string())?.is_empty());
    Ok(())
}

#[test]
fn plugin_logging_has_a_fixed_record_budget() -> Result<(), Box<dyn std::error::Error>> {
    let logs = RecordingLogs::default();
    let vm = PluginVm::new(
        manifest(PluginCapabilitySet::only([PluginCapability::Log]))?,
        limits()?,
        logs.clone(),
    )?;

    assert!(matches!(
        vm.load(PluginProgram::new(
            "many-logs.lua",
            "return { on_load = function(context) for i = 1, 65 do context:info('x') end end }",
        )?),
        Err(PluginVmError::LogBudgetExceeded)
    ));
    assert_eq!(logs.0.lock().map_err(|error| error.to_string())?.len(), 64);
    Ok(())
}

#[test]
fn committed_lua_types_match_the_closed_logging_api() {
    let generated = include_str!("../lua-types/mlua-typegen.lua");

    for method in ["debug", "error", "info", "trace", "warn"] {
        assert!(generated.contains(&format!("PluginContext[\"{method}\"]")));
    }
    assert!(!generated.contains("PluginContext[\"log\"]"));
}

#[test]
fn granted_lifecycle_script_emits_a_structured_log() -> Result<(), Box<dyn std::error::Error>> {
    let logs = RecordingLogs::default();
    let vm = PluginVm::new(
        manifest(PluginCapabilitySet::only([PluginCapability::Log]))?,
        limits()?,
        logs.clone(),
    )?;

    vm.load(PluginProgram::new(
        "workspace-labels.lua",
        br#"
            return {
                on_load = function(context)
                    context:info("workspace labels loaded")
                end
            }
        "#,
    )?)?;

    let records = logs.0.lock().map_err(|error| error.to_string())?;
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].plugin().as_str(), "workspace-labels");
    assert_eq!(records[0].level(), PluginLogLevel::Info);
    assert_eq!(records[0].message(), "workspace labels loaded");
    Ok(())
}

#[test]
fn environment_has_no_ambient_native_or_dynamic_code_authority()
-> Result<(), Box<dyn std::error::Error>> {
    let vm = PluginVm::new(
        manifest(PluginCapabilitySet::empty())?,
        limits()?,
        RecordingLogs::default(),
    )?;

    vm.load(PluginProgram::new(
        "authority-probe.lua",
        br"
            return {
                on_load = function(_)
                    assert(io == nil and os == nil and package == nil)
                    assert(require == nil and debug == nil and ffi == nil and jit == nil)
                    assert(dofile == nil and loadfile == nil and load == nil)
                end
            }
        ",
    )?)?;
    Ok(())
}

#[test]
fn absent_capability_denies_the_call_without_reaching_the_sink()
-> Result<(), Box<dyn std::error::Error>> {
    let logs = RecordingLogs::default();
    let vm = PluginVm::new(
        manifest(PluginCapabilitySet::empty())?,
        limits()?,
        logs.clone(),
    )?;
    let result = vm.load(PluginProgram::new(
        "denied.lua",
        br#"return { on_load = function(context) context:warn("no") end }"#,
    )?);
    let Err(error) = result else {
        return Err("missing capability must return an error".into());
    };

    assert!(matches!(
        error,
        PluginVmError::CapabilityDenied(PluginCapability::Log)
    ));
    assert!(logs.0.lock().map_err(|error| error.to_string())?.is_empty());
    Ok(())
}

#[test]
fn infinite_loop_stops_at_the_instruction_budget() -> Result<(), Box<dyn std::error::Error>> {
    let vm = PluginVm::new(
        manifest(PluginCapabilitySet::empty())?,
        PluginLimits::new(
            MemoryBudget::new(2 * 1024 * 1024).ok_or("memory budget must be nonzero")?,
            InstructionBudget::new(10_000).ok_or("instruction budget must be nonzero")?,
        ),
        RecordingLogs::default(),
    )?;
    let result = vm.load(PluginProgram::new(
        "loop.lua",
        br"return { on_load = function(_) while true do end end }",
    )?);
    let Err(error) = result else {
        return Err("infinite loop must return an error".into());
    };

    assert!(matches!(error, PluginVmError::InstructionBudgetExhausted));
    Ok(())
}

#[test]
fn allocation_stops_at_the_vm_memory_budget_without_poisoning_a_new_vm()
-> Result<(), Box<dyn std::error::Error>> {
    let constrained_limits = PluginLimits::new(
        MemoryBudget::new(64 * 1024).ok_or("memory budget must be nonzero")?,
        InstructionBudget::new(1_000_000).ok_or("instruction budget must be nonzero")?,
    );
    let vm = PluginVm::new(
        manifest(PluginCapabilitySet::empty())?,
        constrained_limits,
        RecordingLogs::default(),
    )?;
    let result = vm.load(PluginProgram::new(
        "allocation.lua",
        br#"
            local values = {}
            for index = 1, 100000 do values[index] = string.rep("x", 32) end
            return { on_load = function(_) end }
        "#,
    )?);
    let Err(error) = result else {
        return Err("allocation beyond the memory budget must return an error".into());
    };
    assert!(matches!(error, PluginVmError::MemoryBudgetExceeded));

    let replacement = PluginVm::new(
        manifest(PluginCapabilitySet::empty())?,
        limits()?,
        RecordingLogs::default(),
    )?;
    replacement.load(PluginProgram::new(
        "replacement.lua",
        br"return { on_load = function(_) end }",
    )?)?;
    Ok(())
}
