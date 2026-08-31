use std::sync::Arc;
use std::sync::Mutex;

use komorebi_extensions::InstructionBudget;
use komorebi_extensions::MemoryBudget;
use komorebi_extensions::PluginCapability;
use komorebi_extensions::PluginCapabilitySet;
use komorebi_extensions::PluginId;
use komorebi_extensions::PluginLimits;
use komorebi_extensions::PluginLogLevel;
use komorebi_extensions::PluginManifest;
use komorebi_extensions::PluginOutput;
use komorebi_extensions::PluginOutputSink;
use komorebi_extensions::PluginProgram;
use komorebi_extensions::PluginProgramError;
use komorebi_extensions::PluginVm;
use komorebi_extensions::PluginVmError;

#[derive(Clone, Default)]
struct RecordingOutputs(Arc<Mutex<Vec<PluginOutput>>>);

impl PluginOutputSink for RecordingOutputs {
    fn emit(&self, output: PluginOutput) {
        if let Ok(mut records) = self.0.lock() {
            records.push(output);
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
    let outputs = RecordingOutputs::default();
    let vm = PluginVm::new(
        manifest(PluginCapabilitySet::only([PluginCapability::Log]))?,
        limits()?,
        outputs.clone(),
    )?;
    let oversized_message = "x".repeat(16 * 1024 + 1);
    let source =
        format!("return {{ on_load = function(context) context:info('{oversized_message}') end }}");

    assert!(matches!(
        vm.load(PluginProgram::new("oversized-log.lua", source)?),
        Err(PluginVmError::LogMessageTooLarge)
    ));
    assert!(
        outputs
            .0
            .lock()
            .map_err(|error| error.to_string())?
            .is_empty()
    );
    Ok(())
}

#[test]
fn plugin_logging_has_a_fixed_record_budget() -> Result<(), Box<dyn std::error::Error>> {
    let outputs = RecordingOutputs::default();
    let vm = PluginVm::new(
        manifest(PluginCapabilitySet::only([PluginCapability::Log]))?,
        limits()?,
        outputs.clone(),
    )?;

    assert!(matches!(
        vm.load(PluginProgram::new(
            "many-logs.lua",
            "return { on_load = function(context) for i = 1, 65 do context:info('x') end end }",
        )?),
        Err(PluginVmError::OutputBudgetExceeded)
    ));
    assert_eq!(
        outputs.0.lock().map_err(|error| error.to_string())?.len(),
        64
    );
    Ok(())
}

#[test]
fn committed_lua_types_match_the_closed_plugin_api() {
    let generated = include_str!("../lua-types/mlua-typegen.lua");

    for method in ["debug", "error", "info", "trace", "warn"] {
        assert!(generated.contains(&format!("PluginContext[\"{method}\"]")));
    }
    assert!(!generated.contains("PluginContext[\"log\"]"));
    for method in [
        "action",
        "boolean",
        "choice",
        "color",
        "decimal",
        "entity",
        "invoke",
        "selector",
        "signed",
        "text",
        "unit",
        "unsigned",
        "windows_path",
    ] {
        assert!(generated.contains(&format!("PluginContext[\"{method}\"]")));
    }
    assert!(generated.contains("---@return PluginActionBuilder"));
    assert!(generated.contains("---@return PluginValue"));
    assert!(generated.contains("---@param action PluginActionBuilder"));
    assert!(generated.contains("---@param values PluginValue[]"));
}

#[test]
fn granted_lifecycle_script_emits_a_structured_log() -> Result<(), Box<dyn std::error::Error>> {
    let outputs = RecordingOutputs::default();
    let vm = PluginVm::new(
        manifest(PluginCapabilitySet::only([PluginCapability::Log]))?,
        limits()?,
        outputs.clone(),
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

    let records = outputs.0.lock().map_err(|error| error.to_string())?;
    let [PluginOutput::Log(record)] = records.as_slice() else {
        return Err("expected one structured log output".into());
    };
    assert_eq!(record.plugin().as_str(), "workspace-labels");
    assert_eq!(record.level(), PluginLogLevel::Info);
    assert_eq!(record.message(), "workspace labels loaded");
    Ok(())
}

#[test]
fn environment_has_no_ambient_native_or_dynamic_code_authority()
-> Result<(), Box<dyn std::error::Error>> {
    let vm = PluginVm::new(
        manifest(PluginCapabilitySet::empty())?,
        limits()?,
        RecordingOutputs::default(),
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
    let outputs = RecordingOutputs::default();
    let vm = PluginVm::new(
        manifest(PluginCapabilitySet::empty())?,
        limits()?,
        outputs.clone(),
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
    assert!(
        outputs
            .0
            .lock()
            .map_err(|error| error.to_string())?
            .is_empty()
    );
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
        RecordingOutputs::default(),
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
        RecordingOutputs::default(),
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
        RecordingOutputs::default(),
    )?;
    replacement.load(PluginProgram::new(
        "replacement.lua",
        br"return { on_load = function(_) end }",
    )?)?;
    Ok(())
}

#[test]
fn action_builder_emits_one_typed_intent_with_lossless_windows_path()
-> Result<(), Box<dyn std::error::Error>> {
    let outputs = RecordingOutputs::default();
    let vm = PluginVm::new(
        manifest(PluginCapabilitySet::only([PluginCapability::InvokeAction]))?,
        limits()?,
        outputs.clone(),
    )?;

    vm.load(PluginProgram::new(
        "typed-action.lua",
        br#"
            return {
                on_load = function(context)
                    local action = context:action("open-at-path")
                    action:set("direction", context:choice("left"))
                    action:set("path", context:windows_path({ 67, 58, 92, 55296 }))
                    context:invoke(action)
                end
            }
        "#,
    )?)?;

    let records = outputs.0.lock().map_err(|error| error.to_string())?;
    let [PluginOutput::InvokeAction(request)] = records.as_slice() else {
        return Err("expected one typed action output".into());
    };
    assert_eq!(request.plugin().as_str(), "workspace-labels");
    assert_eq!(request.intent().action().as_str(), "open-at-path");
    let arguments = request.intent().arguments().values();
    assert!(matches!(
        arguments
            .get(&komorebi_protocol::ParameterId::parse("direction")?),
        Some(komorebi_protocol::ActionArgument::Scalar(
            komorebi_protocol::ArgumentScalar::Choice(value)
        )) if value.as_str() == "left"
    ));
    assert!(matches!(
        arguments.get(&komorebi_protocol::ParameterId::parse("path")?),
        Some(komorebi_protocol::ActionArgument::Scalar(
            komorebi_protocol::ArgumentScalar::WindowsPath(value)
        )) if value.units() == [67, 58, 92, 0xd800]
    ));
    Ok(())
}

#[test]
fn action_capability_is_checked_before_an_intent_reaches_the_sink()
-> Result<(), Box<dyn std::error::Error>> {
    let outputs = RecordingOutputs::default();
    let vm = PluginVm::new(
        manifest(PluginCapabilitySet::empty())?,
        limits()?,
        outputs.clone(),
    )?;

    let result = vm.load(PluginProgram::new(
        "denied-action.lua",
        br#"
            return {
                on_load = function(context)
                    context:invoke(context:action("toggle-pause"))
                end
            }
        "#,
    )?);

    assert!(matches!(
        result,
        Err(PluginVmError::CapabilityDenied(
            PluginCapability::InvokeAction
        ))
    ));
    assert!(
        outputs
            .0
            .lock()
            .map_err(|error| error.to_string())?
            .is_empty()
    );
    Ok(())
}
