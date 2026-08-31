use std::{
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context as _, Result};
use clap::{Parser, Subcommand};
use quickjs_plugin_spike::{
    BenchmarkEngine, BenchmarkResult, BenchmarkSettings, HostConfig, PluginHost, PluginRequest,
    Unconfigured, generated_typescript_declarations, run_benchmark,
};
use serde::Serialize;

#[derive(Debug, Parser)]
#[command(name = "quickjs-plugin-spike")]
#[command(about = "Throwaway QuickJS vs LuaJIT plugin-host spike")]
struct Args {
    #[command(subcommand)]
    command: AppCommand,
}

#[derive(Debug, Subcommand)]
enum AppCommand {
    Run {
        entry: PathBuf,
        #[arg(long)]
        root: Option<PathBuf>,
    },
    Types {
        #[arg(long, default_value = "komorebi.d.ts")]
        output: PathBuf,
    },
    Bench {
        #[arg(long, default_value = "results/benchmark.json")]
        output: PathBuf,
        #[arg(long, default_value_t = 3)]
        rounds: usize,
        #[arg(long, default_value_t = 200)]
        warmup: usize,
        #[arg(long, default_value_t = 1_000)]
        samples: usize,
        #[arg(long, default_value_t = 100_000)]
        loop_iterations: i32,
        #[arg(long, default_value_t = 30)]
        reloads: usize,
    },
    #[command(hide = true)]
    BenchWorker {
        #[arg(long, value_enum)]
        engine: BenchmarkEngine,
        #[arg(long)]
        warmup: usize,
        #[arg(long)]
        samples: usize,
        #[arg(long)]
        loop_iterations: i32,
        #[arg(long)]
        reloads: usize,
    },
}

#[derive(Serialize)]
struct BenchmarkSuite {
    schema: &'static str,
    release_build: bool,
    process_isolation: bool,
    order: &'static str,
    runs: Vec<BenchmarkResult>,
}

#[tokio::main]
async fn main() -> Result<()> {
    match Args::parse().command {
        AppCommand::Run { entry, root } => run(entry, root).await,
        AppCommand::Types { output } => write_types(&output),
        AppCommand::Bench {
            output,
            rounds,
            warmup,
            samples,
            loop_iterations,
            reloads,
        } => run_suite(
            &output,
            rounds,
            BenchmarkSettings {
                warmup_invocations: warmup,
                measured_invocations: samples,
                loop_iterations,
                reloads,
                ..BenchmarkSettings::default()
            },
        ),
        AppCommand::BenchWorker {
            engine,
            warmup,
            samples,
            loop_iterations,
            reloads,
        } => {
            let result = run_benchmark(
                engine,
                BenchmarkSettings {
                    warmup_invocations: warmup,
                    measured_invocations: samples,
                    loop_iterations,
                    reloads,
                    ..BenchmarkSettings::default()
                },
            )?;
            println!("{}", serde_json::to_string(&result)?);
            Ok(())
        }
    }
}

async fn run(entry: PathBuf, root: Option<PathBuf>) -> Result<()> {
    let root = root.unwrap_or_else(|| {
        entry
            .parent()
            .map_or_else(|| PathBuf::from("."), PathBuf::from)
    });
    let host = PluginHost::<Unconfigured>::new()
        .configure(HostConfig::for_root(root))
        .context("configure plugin host")?;
    let report = host
        .execute(PluginRequest::new(entry))
        .await
        .context("execute plugin")?;
    println!("{report:#?}");
    Ok(())
}

fn write_types(output: &Path) -> Result<()> {
    std::fs::write(output, generated_typescript_declarations())
        .with_context(|| format!("write generated host types to {}", output.display()))
}

fn run_suite(output: &Path, rounds: usize, settings: BenchmarkSettings) -> Result<()> {
    let executable = std::env::current_exe().context("locate benchmark executable")?;
    let engines = [
        BenchmarkEngine::QuickJs,
        BenchmarkEngine::LuaJitOff,
        BenchmarkEngine::LuaJitOn,
    ];
    let mut runs = Vec::with_capacity(rounds.saturating_mul(engines.len()));
    for round in 0..rounds {
        for offset in 0..engines.len() {
            let engine = engines[(round + offset) % engines.len()];
            let child = Command::new(&executable)
                .arg("bench-worker")
                .arg("--engine")
                .arg(engine_name(engine))
                .arg("--warmup")
                .arg(settings.warmup_invocations.to_string())
                .arg("--samples")
                .arg(settings.measured_invocations.to_string())
                .arg("--loop-iterations")
                .arg(settings.loop_iterations.to_string())
                .arg("--reloads")
                .arg(settings.reloads.to_string())
                .output()
                .with_context(|| format!("run {engine:?} benchmark worker"))?;
            anyhow::ensure!(
                child.status.success(),
                "{engine:?} benchmark worker failed: {}",
                String::from_utf8_lossy(&child.stderr)
            );
            runs.push(
                serde_json::from_slice(&child.stdout)
                    .with_context(|| format!("decode {engine:?} benchmark result"))?,
            );
        }
    }
    let suite = BenchmarkSuite {
        schema: "quickjs-luajit-plugin-benchmark/v1",
        release_build: !cfg!(debug_assertions),
        process_isolation: true,
        order: "three-engine Latin rotation per round",
        runs,
    };
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create results directory {}", parent.display()))?;
    }
    std::fs::write(output, serde_json::to_vec_pretty(&suite)?)
        .with_context(|| format!("write benchmark results to {}", output.display()))?;
    println!(
        "wrote {} benchmark runs to {}",
        suite.runs.len(),
        output.display()
    );
    Ok(())
}

const fn engine_name(engine: BenchmarkEngine) -> &'static str {
    match engine {
        BenchmarkEngine::QuickJs => "quick-js",
        BenchmarkEngine::LuaJitOff => "lua-jit-off",
        BenchmarkEngine::LuaJitOn => "lua-jit-on",
    }
}
