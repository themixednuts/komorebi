use std::{
    mem::size_of,
    path::{Path, PathBuf},
    time::Instant,
};

use anyhow::{Context as _, Result};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};

mod lua;
mod proof;
mod quickjs;

const EVENTS: usize = 10;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum BenchmarkEngine {
    QuickJs,
    LuaJitOff,
    LuaJitOn,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkloadProof {
    pub checksum: i64,
    pub actions: Vec<String>,
    pub snapshot: i64,
}

/// Runs the black-box correctness workload for one engine mode.
///
/// # Errors
///
/// Returns an error if the engine cannot load or execute the fixture.
pub fn run_workload_proof(engine: BenchmarkEngine) -> Result<WorkloadProof> {
    match engine {
        BenchmarkEngine::QuickJs => proof::quickjs(),
        BenchmarkEngine::LuaJitOff => proof::lua(false),
        BenchmarkEngine::LuaJitOn => proof::lua(true),
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub struct BenchmarkSettings {
    pub warmup_invocations: usize,
    pub measured_invocations: usize,
    pub loop_iterations: i32,
    pub reloads: usize,
    pub incremental_instances: usize,
}

impl Default for BenchmarkSettings {
    fn default() -> Self {
        Self {
            warmup_invocations: 200,
            measured_invocations: 1_000,
            loop_iterations: 100_000,
            reloads: 30,
            incremental_instances: 16,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BenchmarkResult {
    pub engine: BenchmarkEngine,
    pub settings: BenchmarkSettings,
    pub environment: BenchmarkEnvironment,
    pub fixture: FixtureSize,
    pub correctness: WorkloadProof,
    pub stages_ns: StageTimings,
    pub warm_invocation_ns: Vec<u64>,
    pub pure_script_loop: LoopMeasurement,
    pub host_call_loop: LoopMeasurement,
    pub hot_reload_ns: Vec<u64>,
    pub final_reload_state: i64,
    pub memory: MemoryMeasurements,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BenchmarkEnvironment {
    pub architecture: String,
    pub logical_cpus: usize,
    pub affinity_cpu_zero_applied: bool,
    pub above_normal_priority_applied: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FixtureSize {
    pub authored_lines: usize,
    pub authored_bytes: usize,
    pub rust_host_glue_lines: usize,
    pub rust_host_glue_bytes: usize,
    pub generated_source_map_bytes: usize,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct StageTimings {
    pub source_load: u64,
    pub typescript_transpile: u64,
    pub diagnostic_render: u64,
    pub engine_initialization: u64,
    pub context_initialization: u64,
    pub source_compile: u64,
    pub plugin_instantiation: u64,
    pub first_invocation: u64,
    pub teardown: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct LoopMeasurement {
    pub iterations: i32,
    pub elapsed_ns: u64,
    pub output: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MemoryMeasurements {
    pub process_working_set_before: u64,
    pub process_working_set_loaded: u64,
    pub process_working_set_after_teardown: u64,
    pub empty_instance_incremental_bytes: i64,
    pub repeated_reload_growth_bytes: i64,
}

/// Measures one engine mode in the current isolated worker process.
///
/// # Errors
///
/// Returns an error if setup, measurement, correctness validation, or memory sampling fails.
pub fn run_benchmark(
    engine: BenchmarkEngine,
    settings: BenchmarkSettings,
) -> Result<BenchmarkResult> {
    let environment = stabilize_worker();
    let correctness = run_workload_proof(engine)?;
    match engine {
        BenchmarkEngine::QuickJs => quickjs::benchmark(settings, environment, correctness),
        BenchmarkEngine::LuaJitOff => {
            lua::benchmark(false, engine, settings, environment, correctness)
        }
        BenchmarkEngine::LuaJitOn => {
            lua::benchmark(true, engine, settings, environment, correctness)
        }
    }
}

struct ExecutionMeasurements {
    first_invocation_ns: u64,
    warm_invocation_ns: Vec<u64>,
    pure_script_loop: LoopMeasurement,
    host_call_loop: LoopMeasurement,
}

struct ReloadMeasurements {
    samples_ns: Vec<u64>,
    final_state: i64,
    working_set_growth_bytes: i64,
}

fn measure_reloads(
    reloads: usize,
    mut reload_once: impl FnMut(i64) -> Result<i64>,
) -> Result<ReloadMeasurements> {
    let working_set_before = working_set_bytes()?;
    let mut state = 0_i64;
    let mut samples_ns = Vec::with_capacity(reloads);
    for _ in 0..reloads {
        let started = Instant::now();
        state = reload_once(state)?;
        samples_ns.push(nanos(started.elapsed()));
    }
    Ok(ReloadMeasurements {
        samples_ns,
        final_state: state,
        working_set_growth_bytes: signed_delta(working_set_bytes()?, working_set_before),
    })
}

fn read_sources(root: &Path, names: &[&str]) -> Result<Vec<(PathBuf, String)>> {
    names
        .iter()
        .map(|name| {
            let path = root.join(name);
            std::fs::read_to_string(&path)
                .with_context(|| format!("read benchmark fixture {}", path.display()))
                .map(|source| (path, source))
        })
        .collect()
}

fn source_named<'a>(sources: &'a [(PathBuf, String)], name: &str) -> Result<&'a str> {
    sources
        .iter()
        .find(|(path, _)| path.file_name().is_some_and(|file_name| file_name == name))
        .map(|(_, source)| source.as_str())
        .with_context(|| format!("benchmark source {name} is missing"))
}

fn fixture_size(sources: &[(PathBuf, String)]) -> (usize, usize) {
    sources.iter().fold((0, 0), |(lines, bytes), (_, source)| {
        (lines + source.lines().count(), bytes + source.len())
    })
}

fn host_glue_size() -> (usize, usize) {
    const SOURCES: [&str; 4] = [
        include_str!("host.rs"),
        include_str!("module_loader.rs"),
        include_str!("path_key.rs"),
        include_str!("transpile.rs"),
    ];
    SOURCES.iter().fold((0, 0), |(lines, bytes), source| {
        (lines + source.lines().count(), bytes + source.len())
    })
}

fn nanos(duration: std::time::Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}

fn signed_delta(after: u64, before: u64) -> i64 {
    let delta = i128::from(after) - i128::from(before);
    i64::try_from(delta).unwrap_or_else(|_| {
        if delta.is_negative() {
            i64::MIN
        } else {
            i64::MAX
        }
    })
}

fn stabilize_worker() -> BenchmarkEnvironment {
    use windows_sys::Win32::System::Threading::{
        ABOVE_NORMAL_PRIORITY_CLASS, GetCurrentProcess, GetCurrentThread, SetPriorityClass,
        SetThreadAffinityMask,
    };

    // SAFETY: pseudo handles need no closing, affinity mask 1 selects CPU 0, and both calls only
    // mutate scheduling policy for this benchmark process/thread.
    let (affinity, priority) = unsafe {
        (
            SetThreadAffinityMask(GetCurrentThread(), 1) != 0,
            SetPriorityClass(GetCurrentProcess(), ABOVE_NORMAL_PRIORITY_CLASS) != 0,
        )
    };
    BenchmarkEnvironment {
        architecture: std::env::consts::ARCH.to_owned(),
        logical_cpus: std::thread::available_parallelism().map_or(1, usize::from),
        affinity_cpu_zero_applied: affinity,
        above_normal_priority_applied: priority,
    }
}

fn working_set_bytes() -> Result<u64> {
    use windows_sys::Win32::System::{
        ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS},
        Threading::GetCurrentProcess,
    };

    let mut counters = PROCESS_MEMORY_COUNTERS {
        cb: u32::try_from(size_of::<PROCESS_MEMORY_COUNTERS>())?,
        PageFaultCount: 0,
        PeakWorkingSetSize: 0,
        WorkingSetSize: 0,
        QuotaPeakPagedPoolUsage: 0,
        QuotaPagedPoolUsage: 0,
        QuotaPeakNonPagedPoolUsage: 0,
        QuotaNonPagedPoolUsage: 0,
        PagefileUsage: 0,
        PeakPagefileUsage: 0,
    };
    // SAFETY: counters points to writable storage of the declared size and the pseudo process
    // handle remains valid for the duration of the call.
    let succeeded = unsafe {
        K32GetProcessMemoryInfo(
            GetCurrentProcess(),
            &raw mut counters,
            u32::try_from(size_of::<PROCESS_MEMORY_COUNTERS>())?,
        )
    };
    anyhow::ensure!(succeeded != 0, "K32GetProcessMemoryInfo failed");
    Ok(u64::try_from(counters.WorkingSetSize)?)
}

fn fixture_root(language: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(language)
}
