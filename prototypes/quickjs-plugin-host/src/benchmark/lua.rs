use std::{
    hint::black_box,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Instant,
};

use anyhow::Result;
use mlua::{Function as LuaFunction, Lua, Table as LuaTable};

use super::{
    BenchmarkEngine, BenchmarkEnvironment, BenchmarkResult, BenchmarkSettings,
    ExecutionMeasurements, FixtureSize, LoopMeasurement, MemoryMeasurements, StageTimings,
    WorkloadProof, fixture_root, fixture_size, host_glue_size, measure_reloads, nanos,
    read_sources, signed_delta, source_named, working_set_bytes,
};

pub(super) fn benchmark(
    jit: bool,
    engine: BenchmarkEngine,
    settings: BenchmarkSettings,
    environment: BenchmarkEnvironment,
    correctness: WorkloadProof,
) -> Result<BenchmarkResult> {
    let root = fixture_root("lua");
    let source_started = Instant::now();
    let sources = read_sources(&root, &["plugin.lua", "scoring.lua"])?;
    let source_load = nanos(source_started.elapsed());
    let authored = fixture_size(&sources);
    let scoring_source = source_named(&sources, "scoring.lua")?;
    let plugin_source = source_named(&sources, "plugin.lua")?;
    let working_set_before = working_set_bytes()?;

    let engine_started = Instant::now();
    let lua = Lua::new();
    configure_jit(&lua, jit)?;
    let engine_initialization = nanos(engine_started.elapsed());
    let diagnostic_started = Instant::now();
    let diagnostic = lua.load("local broken = ").into_function();
    anyhow::ensure!(diagnostic.is_err(), "invalid Lua unexpectedly compiled");
    let diagnostic_render = nanos(diagnostic_started.elapsed());
    let host_calls = Arc::new(AtomicU64::new(0));
    let callback_calls = Arc::clone(&host_calls);
    lua.globals().set(
        "focus",
        lua.create_function(move |_, _value: String| {
            callback_calls.fetch_add(1, Ordering::Relaxed);
            Ok(())
        })?,
    )?;
    let compile_started = Instant::now();
    let scoring_chunk = lua
        .load(scoring_source)
        .set_name("@scoring.lua")
        .into_function()?;
    let plugin_chunk = lua
        .load(plugin_source)
        .set_name("@plugin.lua")
        .into_function()?;
    let source_compile = nanos(compile_started.elapsed());
    let instantiate_started = Instant::now();
    let package: LuaTable = lua.globals().get("package")?;
    let preload: LuaTable = package.get("preload")?;
    preload.set("scoring", scoring_chunk)?;
    let plugin: LuaTable = plugin_chunk.call(())?;
    let plugin_instantiation = nanos(instantiate_started.elapsed());
    let invoke: LuaFunction = plugin.get("invoke")?;
    let pure_loop: LuaFunction = plugin.get("pure_loop")?;
    let host_loop: LuaFunction = plugin.get("host_loop")?;
    let execution = measure_calls(&lua, &invoke, &pure_loop, &host_loop, &host_calls, settings)?;
    let working_set_loaded = working_set_bytes()?;
    let teardown_started = Instant::now();
    drop(host_loop);
    drop(pure_loop);
    drop(invoke);
    drop(plugin);
    drop(preload);
    drop(package);
    drop(lua);
    let teardown = nanos(teardown_started.elapsed());
    let working_set_after_teardown = working_set_bytes()?;
    let incremental = incremental_memory(jit, settings.incremental_instances)?;
    let reload = measure_reloads(settings.reloads, |state| {
        reload_once(jit, scoring_source, plugin_source, state)
    })?;

    Ok(BenchmarkResult {
        engine,
        settings,
        environment,
        fixture: FixtureSize {
            authored_lines: authored.0,
            authored_bytes: authored.1,
            rust_host_glue_lines: host_glue_size().0,
            rust_host_glue_bytes: host_glue_size().1,
            generated_source_map_bytes: 0,
        },
        correctness,
        stages_ns: StageTimings {
            source_load,
            typescript_transpile: 0,
            diagnostic_render,
            engine_initialization,
            context_initialization: 0,
            source_compile,
            plugin_instantiation,
            first_invocation: execution.first_invocation_ns,
            teardown,
        },
        warm_invocation_ns: execution.warm_invocation_ns,
        pure_script_loop: execution.pure_script_loop,
        host_call_loop: execution.host_call_loop,
        hot_reload_ns: reload.samples_ns,
        final_reload_state: reload.final_state,
        memory: MemoryMeasurements {
            process_working_set_before: working_set_before,
            process_working_set_loaded: working_set_loaded,
            process_working_set_after_teardown: working_set_after_teardown,
            empty_instance_incremental_bytes: incremental,
            repeated_reload_growth_bytes: reload.working_set_growth_bytes,
        },
    })
}

fn measure_calls(
    lua: &Lua,
    invoke: &LuaFunction,
    pure_loop: &LuaFunction,
    host_loop: &LuaFunction,
    host_calls: &AtomicU64,
    settings: BenchmarkSettings,
) -> Result<ExecutionMeasurements> {
    let first_started = Instant::now();
    black_box(invoke.call::<i64>(event(lua, 0)?)?);
    let first_invocation_ns = nanos(first_started.elapsed());
    for index in 0..settings.warmup_invocations {
        black_box(invoke.call::<i64>(event(lua, index)?)?);
    }
    let mut warm_invocation_ns = Vec::with_capacity(settings.measured_invocations);
    for index in 0..settings.measured_invocations {
        let started = Instant::now();
        black_box(invoke.call::<i64>(event(lua, index)?)?);
        warm_invocation_ns.push(nanos(started.elapsed()));
    }
    let pure_started = Instant::now();
    let pure_output = pure_loop.call::<i64>(settings.loop_iterations)?;
    let pure_script_loop = LoopMeasurement {
        iterations: settings.loop_iterations,
        elapsed_ns: nanos(pure_started.elapsed()),
        output: black_box(pure_output),
    };
    host_calls.store(0, Ordering::Relaxed);
    let host_started = Instant::now();
    let host_output = host_loop.call::<i64>(settings.loop_iterations)?;
    let host_call_loop = LoopMeasurement {
        iterations: settings.loop_iterations,
        elapsed_ns: nanos(host_started.elapsed()),
        output: black_box(host_output),
    };
    anyhow::ensure!(
        host_calls.load(Ordering::Relaxed) == u64::try_from(settings.loop_iterations)?,
        "Lua host loop skipped callbacks"
    );
    Ok(ExecutionMeasurements {
        first_invocation_ns,
        warm_invocation_ns,
        pure_script_loop,
        host_call_loop,
    })
}

fn event(lua: &Lua, index: usize) -> Result<LuaTable> {
    let event = lua.create_table()?;
    event.set("window_id", i64::try_from(index % 10 + 1)?)?;
    event.set("workspace", i64::try_from(index % 4)?)?;
    Ok(event)
}

fn reload_once(jit: bool, scoring: &str, plugin: &str, state: i64) -> Result<i64> {
    let lua = Lua::new();
    configure_jit(&lua, jit)?;
    lua.globals()
        .set("focus", lua.create_function(|_, _value: String| Ok(()))?)?;
    let package: LuaTable = lua.globals().get("package")?;
    let preload: LuaTable = package.get("preload")?;
    preload.set(
        "scoring",
        lua.load(scoring).set_name("@scoring.lua").into_function()?,
    )?;
    let loaded: LuaTable = lua.load(plugin).set_name("@plugin.lua").eval()?;
    loaded.get::<LuaFunction>("restore")?.call::<()>(state)?;
    black_box(
        loaded
            .get::<LuaFunction>("invoke")?
            .call::<i64>(event(&lua, usize::try_from(state)?)?)?,
    );
    Ok(loaded.get::<LuaFunction>("snapshot")?.call(())?)
}

fn incremental_memory(jit: bool, instances: usize) -> Result<i64> {
    let before = working_set_bytes()?;
    let mut engines = Vec::with_capacity(instances);
    for _ in 0..instances {
        let lua = Lua::new();
        configure_jit(&lua, jit)?;
        engines.push(lua);
    }
    let after = working_set_bytes()?;
    let delta = signed_delta(after, before);
    drop(engines);
    Ok(delta / i64::try_from(instances.max(1))?)
}

pub(super) fn configure_jit(lua: &Lua, enabled: bool) -> mlua::Result<()> {
    lua.load(if enabled {
        "jit.flush(); jit.on()"
    } else {
        "jit.off(); jit.flush()"
    })
    .exec()
}
