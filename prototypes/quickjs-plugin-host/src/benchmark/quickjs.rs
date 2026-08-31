use std::{
    collections::HashMap,
    hint::black_box,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Instant,
};

use anyhow::{Context as _, Result};
use rquickjs::{CatchResultExt, Context, Function, Module, Object, Runtime};

use crate::{
    module_loader::{ModuleTelemetry, PluginLoader, PluginResolver},
    path_key, transpile,
};

use super::{
    BenchmarkEngine, BenchmarkEnvironment, BenchmarkResult, BenchmarkSettings,
    ExecutionMeasurements, FixtureSize, LoopMeasurement, MemoryMeasurements, StageTimings,
    WorkloadProof, fixture_root, fixture_size, host_glue_size, measure_reloads, nanos,
    read_sources, signed_delta, working_set_bytes,
};

struct Sources {
    root: PathBuf,
    source_load_ns: u64,
    authored: (usize, usize),
    precompiled: HashMap<String, (String, String)>,
    modules_to_compile: Vec<(String, String)>,
    transpile_ns: u64,
    diagnostic_ns: u64,
    source_map_bytes: usize,
}

fn prepare_sources() -> Result<Sources> {
    let root = fixture_root("typescript").canonicalize()?;
    let source_started = Instant::now();
    let sources = read_sources(&root, &["plugin.ts", "scoring.ts"])?;
    let source_load_ns = nanos(source_started.elapsed());
    let authored = fixture_size(&sources);
    let transform_started = Instant::now();
    let mut precompiled = HashMap::new();
    let mut source_map_bytes = 0;
    for (path, source) in &sources {
        let canonical = path.canonicalize()?;
        let module_name = path_key::encode(&canonical);
        let output = transpile::typescript(&canonical, &module_name, source)?;
        source_map_bytes += output.source_map.len();
        precompiled.insert(module_name, (output.code, output.source_map));
    }
    let mut modules_to_compile = precompiled
        .iter()
        .map(|(name, (code, _))| (name.clone(), code.clone()))
        .collect::<Vec<_>>();
    modules_to_compile.sort_unstable_by(|left, right| left.0.cmp(&right.0));
    let transpile_ns = nanos(transform_started.elapsed());
    let diagnostic_started = Instant::now();
    let diagnostic = transpile::typescript(
        &root.join("diagnostic.ts"),
        "diagnostic.ts",
        "const broken: = ;",
    );
    anyhow::ensure!(
        diagnostic.is_err(),
        "invalid TypeScript unexpectedly transpiled"
    );
    Ok(Sources {
        root,
        source_load_ns,
        authored,
        precompiled,
        modules_to_compile,
        transpile_ns,
        diagnostic_ns: nanos(diagnostic_started.elapsed()),
        source_map_bytes,
    })
}

pub(super) fn benchmark(
    settings: BenchmarkSettings,
    environment: BenchmarkEnvironment,
    correctness: WorkloadProof,
) -> Result<BenchmarkResult> {
    let sources = prepare_sources()?;
    let working_set_before = working_set_bytes()?;
    let engine_started = Instant::now();
    let runtime = Runtime::new().context("create QuickJS benchmark runtime")?;
    let engine_initialization = nanos(engine_started.elapsed());
    let telemetry = Arc::new(ModuleTelemetry::default());
    runtime.set_loader(
        PluginResolver::new(sources.root.clone()),
        PluginLoader::with_precompiled(telemetry, sources.precompiled),
    );
    let context_started = Instant::now();
    let context = Context::full(&runtime).context("create QuickJS benchmark context")?;
    let context_initialization = nanos(context_started.elapsed());
    let entry = sources.root.join("plugin.ts").canonicalize()?;
    let host_calls = Arc::new(AtomicU64::new(0));
    let callback_calls = Arc::clone(&host_calls);
    let (execution, source_compile, plugin_instantiation) = context.with(|ctx| -> Result<_> {
        let compile_started = Instant::now();
        for (name, code) in sources.modules_to_compile {
            black_box(Module::declare(ctx.clone(), name, code)?);
        }
        let source_compile = nanos(compile_started.elapsed());
        let focus = Function::new(ctx.clone(), move |_value: String| {
            callback_calls.fetch_add(1, Ordering::Relaxed);
        })?;
        ctx.globals().set("__komorebi_focus", focus)?;
        let instantiation_started = Instant::now();
        let namespace = Module::import(&ctx, path_key::encode(&entry))
            .catch(&ctx)
            .map_err(|error| anyhow::anyhow!(error.to_string()))?
            .finish::<Object>()
            .catch(&ctx)
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        let plugin_instantiation = nanos(instantiation_started.elapsed());
        let invoke: Function = namespace.get("invoke")?;
        let pure_loop: Function = namespace.get("pureLoop")?;
        let host_loop: Function = namespace.get("hostLoop")?;
        let execution =
            measure_calls(&ctx, &invoke, &pure_loop, &host_loop, &host_calls, settings)?;
        Ok((execution, source_compile, plugin_instantiation))
    })?;
    let working_set_loaded = working_set_bytes()?;
    let teardown_started = Instant::now();
    drop(context);
    drop(runtime);
    let teardown = nanos(teardown_started.elapsed());
    let working_set_after_teardown = working_set_bytes()?;
    let incremental = incremental_memory(settings.incremental_instances)?;
    let reload = measure_reloads(settings.reloads, |state| reload_once(&sources.root, state))?;

    Ok(BenchmarkResult {
        engine: BenchmarkEngine::QuickJs,
        settings,
        environment,
        fixture: FixtureSize {
            authored_lines: sources.authored.0,
            authored_bytes: sources.authored.1,
            rust_host_glue_lines: host_glue_size().0,
            rust_host_glue_bytes: host_glue_size().1,
            generated_source_map_bytes: sources.source_map_bytes,
        },
        correctness,
        stages_ns: StageTimings {
            source_load: sources.source_load_ns,
            typescript_transpile: sources.transpile_ns,
            diagnostic_render: sources.diagnostic_ns,
            engine_initialization,
            context_initialization,
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

fn measure_calls<'js>(
    ctx: &rquickjs::Ctx<'js>,
    invoke: &Function<'js>,
    pure_loop: &Function<'js>,
    host_loop: &Function<'js>,
    host_calls: &AtomicU64,
    settings: BenchmarkSettings,
) -> Result<ExecutionMeasurements> {
    let first_started = Instant::now();
    black_box(invoke.call::<_, i64>((event(ctx, 0)?,))?);
    let first_invocation_ns = nanos(first_started.elapsed());
    for index in 0..settings.warmup_invocations {
        black_box(invoke.call::<_, i64>((event(ctx, index)?,))?);
    }
    let mut warm_invocation_ns = Vec::with_capacity(settings.measured_invocations);
    for index in 0..settings.measured_invocations {
        let started = Instant::now();
        black_box(invoke.call::<_, i64>((event(ctx, index)?,))?);
        warm_invocation_ns.push(nanos(started.elapsed()));
    }
    let pure_started = Instant::now();
    let pure_output = pure_loop.call::<_, i64>((settings.loop_iterations,))?;
    let pure_script_loop = LoopMeasurement {
        iterations: settings.loop_iterations,
        elapsed_ns: nanos(pure_started.elapsed()),
        output: black_box(pure_output),
    };
    host_calls.store(0, Ordering::Relaxed);
    let host_started = Instant::now();
    let host_output = host_loop.call::<_, i64>((settings.loop_iterations,))?;
    let host_call_loop = LoopMeasurement {
        iterations: settings.loop_iterations,
        elapsed_ns: nanos(host_started.elapsed()),
        output: black_box(host_output),
    };
    anyhow::ensure!(
        host_calls.load(Ordering::Relaxed) == u64::try_from(settings.loop_iterations)?,
        "QuickJS host loop skipped callbacks"
    );
    Ok(ExecutionMeasurements {
        first_invocation_ns,
        warm_invocation_ns,
        pure_script_loop,
        host_call_loop,
    })
}

fn event<'js>(ctx: &rquickjs::Ctx<'js>, index: usize) -> Result<Object<'js>> {
    let event = Object::new(ctx.clone())?;
    event.set("windowId", i32::try_from(index % 10 + 1)?)?;
    event.set("workspace", i32::try_from(index % 4)?)?;
    Ok(event)
}

fn reload_once(root: &Path, state: i64) -> Result<i64> {
    let runtime = Runtime::new()?;
    runtime.set_loader(
        PluginResolver::new(root.to_path_buf()),
        PluginLoader::new(Arc::new(ModuleTelemetry::default())),
    );
    let context = Context::full(&runtime)?;
    let entry = root.join("plugin.ts").canonicalize()?;
    context.with(|ctx| -> Result<i64> {
        ctx.globals().set(
            "__komorebi_focus",
            Function::new(ctx.clone(), |_value: String| {})?,
        )?;
        let namespace = Module::import(&ctx, path_key::encode(&entry))
            .catch(&ctx)
            .map_err(|error| anyhow::anyhow!(error.to_string()))?
            .finish::<Object>()
            .catch(&ctx)
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        namespace
            .get::<_, Function>("restore")?
            .call::<_, ()>((state,))?;
        black_box(
            namespace
                .get::<_, Function>("invoke")?
                .call::<_, i64>((event(&ctx, usize::try_from(state)?)?,))?,
        );
        Ok(namespace.get::<_, Function>("snapshot")?.call(())?)
    })
}

fn incremental_memory(instances: usize) -> Result<i64> {
    let before = working_set_bytes()?;
    let mut engines = Vec::with_capacity(instances);
    for _ in 0..instances {
        let runtime = Runtime::new()?;
        let context = Context::full(&runtime)?;
        engines.push((runtime, context));
    }
    let after = working_set_bytes()?;
    let delta = signed_delta(after, before);
    drop(engines);
    Ok(delta / i64::try_from(instances.max(1))?)
}
