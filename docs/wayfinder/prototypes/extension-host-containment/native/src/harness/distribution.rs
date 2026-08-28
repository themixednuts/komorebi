use std::path::Path;
use std::thread::JoinHandle;
use std::time::Instant;

use anyhow::{Context, Result, anyhow, bail, ensure};

use crate::protocol::{ExpectedOutcome, ExtensionWorkload, ObservedOutcome, RuntimeKind};

use super::policy::ContainmentPolicy;
use super::report::{LaunchCohortDistribution, LaunchDistributionEvidence, RunReport, ScaleReport};
use super::{percentile, percentile_index, run_extension};

pub(super) fn run(
    executable: &Path,
    private_file: &Path,
    policy: &ContainmentPolicy,
) -> Result<(Vec<ScaleReport>, LaunchDistributionEvidence)> {
    let repetitions = policy.workload().launch_distribution_repetitions();
    let mut latest = Vec::new();
    let mut cohorts = Vec::new();
    for process_count in policy.workload().cohort_sizes() {
        let mut samples = Vec::with_capacity(repetitions);
        for _ in 0..repetitions {
            samples.push(run_cohort(process_count, executable, private_file, policy)?);
        }
        latest.push(
            samples
                .last()
                .cloned()
                .context("launch distribution produced no latest sample")?,
        );
        cohorts.push(summarize(process_count, samples)?);
    }
    Ok((
        latest,
        LaunchDistributionEvidence {
            repetitions_per_cohort: repetitions,
            profile_condition: "fresh AppContainer profile and process for every sample",
            os_cache_condition: "uncontrolled resident Windows file and image cache",
            cold_launch_status: "not claimed: an unattended run has no reboot or safe process-local OS cache reset boundary",
            warm_launch_status: "measured: immediate repeats after the first observed cohort",
            cohorts,
        },
    ))
}

fn run_cohort(
    process_count: usize,
    executable: &Path,
    private_file: &Path,
    policy: &ContainmentPolicy,
) -> Result<ScaleReport> {
    let cohort_started = Instant::now();
    let mut workers = Vec::with_capacity(process_count);
    let mut failures = Vec::new();
    for index in 0..process_count {
        let executable = executable.to_path_buf();
        let private_file = private_file.to_path_buf();
        let policy = policy.clone();
        let worker = std::thread::Builder::new()
            .name(format!("scale-extension-{index}"))
            .spawn(move || {
                run_extension(
                    RuntimeKind::Rust,
                    &executable,
                    &private_file,
                    &policy,
                    ExtensionWorkload::LaunchScale,
                )
            });
        match worker {
            Ok(worker) => workers.push(worker),
            Err(error) => {
                failures.push(anyhow!(error).context("spawn scale extension worker"));
                break;
            }
        }
    }

    let mut runs = Vec::with_capacity(workers.len());
    for worker in workers {
        match join_worker(worker) {
            Ok(run) => runs.push(run),
            Err(error) => failures.push(error),
        }
    }
    if !failures.is_empty() {
        let detail = failures
            .iter()
            .enumerate()
            .map(|(index, error)| format!("{}: {error:#}", index + 1))
            .collect::<Vec<_>>()
            .join("; ");
        bail!("scale cohort failed: {detail}");
    }
    ensure!(
        runs.len() == process_count,
        "scale cohort did not launch every requested process"
    );

    let mut ready: Vec<_> = runs.iter().map(|run| run.startup_ms).collect();
    let mut rtt: Vec<_> = runs
        .iter()
        .flat_map(|run| run.echo_rtt_us.iter().copied())
        .collect();
    ready.sort_by(f64::total_cmp);
    rtt.sort_by(f64::total_cmp);
    let aggregate_private_commit_bytes = runs.iter().try_fold(0_usize, |total, run| {
        total
            .checked_add(run.private_commit_bytes)
            .context("aggregate private-commit overflow")
    })?;
    Ok(ScaleReport {
        process_count,
        cohort_wall_ms: cohort_started.elapsed().as_secs_f64() * 1_000.0,
        authenticated_ready_p50_ms: percentile(&ready, 50, 100),
        authenticated_ready_p99_ms: percentile(&ready, 99, 100),
        aggregate_private_commit_bytes,
        echo_rtt_p99_us: percentile(&rtt, 99, 100),
        forbidden_probes_allowed: runs
            .iter()
            .flat_map(|run| &run.probes)
            .filter(|probe| {
                matches!(probe.expected, ExpectedOutcome::Denied)
                    && matches!(probe.observed, ObservedOutcome::Allowed)
            })
            .count(),
        all_exited: runs.iter().all(|run| run.exit_observed.passed()),
    })
}

fn summarize(process_count: usize, samples: Vec<ScaleReport>) -> Result<LaunchCohortDistribution> {
    let mut samples = samples.into_iter();
    let first_observed = samples.next().context("missing first launch observation")?;
    let warm_samples: Vec<_> = samples.collect();
    ensure!(!warm_samples.is_empty(), "missing warm launch samples");
    let mut wall: Vec<_> = warm_samples
        .iter()
        .map(|sample| sample.cohort_wall_ms)
        .collect();
    let mut ready: Vec<_> = warm_samples
        .iter()
        .map(|sample| sample.authenticated_ready_p99_ms)
        .collect();
    let mut echo: Vec<_> = warm_samples
        .iter()
        .map(|sample| sample.echo_rtt_p99_us)
        .collect();
    let mut commit: Vec<_> = warm_samples
        .iter()
        .map(|sample| sample.aggregate_private_commit_bytes)
        .collect();
    wall.sort_by(f64::total_cmp);
    ready.sort_by(f64::total_cmp);
    echo.sort_by(f64::total_cmp);
    commit.sort_unstable();
    let commit_index =
        percentile_index(commit.len(), 99, 100).context("missing warm private-commit sample")?;
    let warm_aggregate_private_commit_p99_bytes = commit
        .get(commit_index)
        .copied()
        .context("warm private-commit percentile is out of bounds")?;
    let forbidden_probes_allowed = warm_samples.iter().try_fold(0_usize, |total, sample| {
        total
            .checked_add(sample.forbidden_probes_allowed)
            .context("forbidden-probe count overflow")
    })?;
    let all_exited = warm_samples.iter().all(|sample| sample.all_exited);
    ensure!(
        forbidden_probes_allowed == 0 && all_exited,
        "a warm launch cohort violated containment or cleanup"
    );
    Ok(LaunchCohortDistribution {
        process_count,
        first_observed,
        warm_cohort_wall_p50_ms: percentile(&wall, 50, 100),
        warm_cohort_wall_p99_ms: percentile(&wall, 99, 100),
        warm_authenticated_ready_p99_of_samples_ms: percentile(&ready, 99, 100),
        warm_echo_p99_of_samples_us: percentile(&echo, 99, 100),
        warm_aggregate_private_commit_p99_bytes,
        forbidden_probes_allowed,
        all_exited,
        warm_samples,
    })
}

fn join_worker(worker: JoinHandle<Result<RunReport>>) -> Result<RunReport> {
    worker
        .join()
        .map_err(|_| anyhow!("scale extension worker panicked"))?
}
