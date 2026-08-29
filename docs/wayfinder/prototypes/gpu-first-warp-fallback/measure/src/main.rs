use std::{
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context as _, Result};
use decoration_compute_core::{
    AdmittedRuntime, FrameBudget, HardwarePreference, MeasuredCost, SceneDevice, admit,
    enforce_budget,
};
use decoration_compute_windows::{ParticleScene, open, probe};
use particle_kernel::{ParticleBatch, ParticleStep};
use serde::Serialize;

const PARTICLES: usize = 2_048;
const FRAMES: u32 = 240;
const BUDGET: FrameBudget = FrameBudget {
    max_update_ns: 4_166_000,
};

#[derive(Serialize)]
struct Report {
    evidence: decoration_compute_core::DeviceEvidence,
    paths: Vec<PathReport>,
    idle_cpu_ns: u64,
}

#[derive(Serialize)]
struct PathReport {
    preference: HardwarePreference,
    runtime: Option<AdmittedRuntime>,
    unavailable: Option<String>,
    p50_ns: Option<u64>,
    p95_ns: Option<u64>,
    p99_ns: Option<u64>,
    mean_ns: Option<u64>,
    within_budget: Option<bool>,
}

fn main() -> Result<()> {
    let evidence = probe().context("probe scene devices")?;
    let step = ParticleStep::checked(1.0 / 240.0, 0.985, 0.0, -9.8).context("step")?;
    let mut paths = Vec::new();
    for preference in [HardwarePreference::Enabled, HardwarePreference::Disabled] {
        paths.push(measure_path(preference, evidence, step)?);
    }
    if evidence.hardware_adapter
        && evidence.hardware_compute
        && let Ok(runtime) = decoration_compute_core::AdmittedRuntime::live(
            SceneDevice::Hardware,
            decoration_compute_core::EffectCompute::CpuUpload,
        )
    {
        paths.push(time_runtime(
            HardwarePreference::Enabled,
            runtime,
            step,
            "forced cpu upload on hardware",
        )?);
    }
    let idle = idle_cpu();
    let report = Report {
        evidence,
        paths,
        idle_cpu_ns: idle,
    };
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

fn measure_path(
    preference: HardwarePreference,
    evidence: decoration_compute_core::DeviceEvidence,
    step: ParticleStep,
) -> Result<PathReport> {
    match admit(preference, evidence) {
        Ok(runtime) => time_runtime(preference, runtime, step, "admitted"),
        Err(reason) => Ok(PathReport {
            preference,
            runtime: None,
            unavailable: Some(reason.to_string()),
            p50_ns: None,
            p95_ns: None,
            p99_ns: None,
            mean_ns: None,
            within_budget: None,
        }),
    }
}

fn time_runtime(
    preference: HardwarePreference,
    runtime: AdmittedRuntime,
    step: ParticleStep,
    _label: &str,
) -> Result<PathReport> {
    let gpu = open(runtime).context("open admitted device")?;
    let mut batch = ParticleBatch::seeded(PARTICLES, 29);
    let scene = ParticleScene::attach(gpu, runtime, &batch).context("attach particles")?;
    let mut samples = Vec::with_capacity(FRAMES as usize);
    for _ in 0..FRAMES {
        let started = Instant::now();
        match runtime {
            AdmittedRuntime::Live {
                compute: decoration_compute_core::EffectCompute::DeviceCompute,
                ..
            } => scene.dispatch(step).context("dispatch")?,
            AdmittedRuntime::Live {
                compute: decoration_compute_core::EffectCompute::CpuUpload,
                ..
            } => scene.step_cpu(&mut batch, step).context("cpu upload")?,
        }
        samples.push(started.elapsed().as_nanos() as u64);
    }
    samples.sort_unstable();
    let p50 = percentile(&samples, 0.50);
    let p95 = percentile(&samples, 0.95);
    let p99 = percentile(&samples, 0.99);
    let mean = samples.iter().sum::<u64>() / samples.len() as u64;
    let within = enforce_budget(runtime, MeasuredCost { update_ns: p95 }, BUDGET).is_ok();
    Ok(PathReport {
        preference,
        runtime: Some(runtime),
        unavailable: None,
        p50_ns: Some(p50),
        p95_ns: Some(p95),
        p99_ns: Some(p99),
        mean_ns: Some(mean),
        within_budget: Some(within),
    })
}

fn percentile(sorted: &[u64], fraction: f64) -> u64 {
    let index = ((sorted.len().saturating_sub(1) as f64) * fraction).round() as usize;
    sorted[index]
}

fn idle_cpu() -> u64 {
    let started = Instant::now();
    thread::park_timeout(Duration::from_millis(200));
    started.elapsed().as_nanos() as u64
}
