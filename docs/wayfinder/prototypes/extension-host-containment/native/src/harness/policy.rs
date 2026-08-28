use std::num::{NonZeroU32, NonZeroUsize};
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use crate::protocol::{ExtensionGeneration, FaultScenario};

mod http;

pub(super) use http::HttpPolicy;
use http::RawHttpPolicy;

#[derive(Debug, Clone)]
pub(super) struct ContainmentPolicy {
    profile_prefix: String,
    compatibility_capabilities: Box<[String]>,
    job: JobPolicy,
    pipe: PipePolicy,
    process: ProcessPolicy,
    http: HttpPolicy,
    workload: WorkloadPolicy,
    faults: FaultPolicy,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct JobPolicy {
    active_process_limit: NonZeroU32,
    memory_limit_bytes: NonZeroUsize,
    cpu_hard_cap_basis_points: NonZeroU32,
    kill_on_close: bool,
    ui_restrictions: bool,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct PipePolicy {
    buffer_bytes: NonZeroU32,
    maximum_frame_bytes: NonZeroUsize,
    connect_timeout: Duration,
    operation_timeout: Duration,
}

#[derive(Debug, Clone)]
pub(super) struct WorkloadPolicy {
    generation: ExtensionGeneration,
    echo_samples: NonZeroUsize,
    cohort_sizes: Box<[NonZeroUsize]>,
    launch_distribution_repetitions: NonZeroUsize,
    nested_job_context_timeout: Duration,
    shared_host_contexts: NonZeroUsize,
    shared_host_noop_samples: NonZeroUsize,
    storage_key_limit_bytes: NonZeroUsize,
    storage_value_limit_bytes: NonZeroUsize,
    storage_entry_limit: NonZeroUsize,
    storage_quota_bytes: NonZeroUsize,
    responsiveness_samples: NonZeroUsize,
    backpressure_payload_bytes: NonZeroUsize,
    backpressure_attempt_limit: NonZeroUsize,
}

#[derive(Debug, Clone)]
pub(super) struct FaultPolicy {
    scenarios: Box<[FaultScenario]>,
    allocation_chunk_bytes: NonZeroUsize,
    termination_exit_code: NonZeroU32,
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub(super) struct ProcessPolicy {
    disable_win32k: bool,
    restrict_child_processes: bool,
    opt_out_all_application_packages: bool,
}

#[derive(Debug, Deserialize)]
struct RawContainmentPolicy {
    profile_prefix: String,
    compatibility_capabilities: Vec<String>,
    job: RawJobPolicy,
    pipe: RawPipePolicy,
    process: ProcessPolicy,
    http: RawHttpPolicy,
    workload: RawWorkloadPolicy,
    faults: RawFaultPolicy,
}

#[derive(Debug, Deserialize)]
struct RawJobPolicy {
    active_process_limit: u32,
    memory_limit_bytes: usize,
    cpu_hard_cap_basis_points: u32,
    kill_on_close: bool,
    ui_restrictions: bool,
}

#[derive(Debug, Deserialize)]
struct RawPipePolicy {
    buffer_bytes: u32,
    maximum_frame_bytes: usize,
    connect_timeout_ms: u32,
    operation_timeout_ms: u32,
}

#[derive(Debug, Deserialize)]
struct RawWorkloadPolicy {
    generation: u64,
    echo_samples: usize,
    cohort_sizes: Vec<usize>,
    launch_distribution_repetitions: usize,
    nested_job_context_timeout_ms: u32,
    shared_host_contexts: usize,
    shared_host_noop_samples: usize,
    storage_key_limit_bytes: usize,
    storage_value_limit_bytes: usize,
    storage_entry_limit: usize,
    storage_quota_bytes: usize,
    responsiveness_samples: usize,
    backpressure_payload_bytes: usize,
    backpressure_attempt_limit: usize,
}

#[derive(Debug, Deserialize)]
struct RawFaultPolicy {
    scenarios: Vec<FaultScenario>,
    allocation_chunk_bytes: usize,
    termination_exit_code: u32,
}

impl ContainmentPolicy {
    pub(super) fn load(path: &Path) -> Result<Self> {
        let source = std::fs::read_to_string(path)
            .with_context(|| format!("read containment policy {}", path.display()))?;
        let raw: RawContainmentPolicy = serde_json::from_str(&source)
            .with_context(|| format!("parse containment policy {}", path.display()))?;
        raw.try_into()
    }

    pub(super) fn profile_prefix(&self) -> &str {
        &self.profile_prefix
    }

    pub(super) fn compatibility_capabilities(&self) -> &[String] {
        &self.compatibility_capabilities
    }

    pub(super) const fn job(&self) -> JobPolicy {
        self.job
    }

    pub(super) const fn pipe(&self) -> PipePolicy {
        self.pipe
    }

    pub(super) const fn process(&self) -> ProcessPolicy {
        self.process
    }

    pub(super) fn http(&self) -> &HttpPolicy {
        &self.http
    }

    pub(super) fn workload(&self) -> &WorkloadPolicy {
        &self.workload
    }

    pub(super) fn faults(&self) -> &FaultPolicy {
        &self.faults
    }
}

impl TryFrom<RawContainmentPolicy> for ContainmentPolicy {
    type Error = anyhow::Error;

    fn try_from(raw: RawContainmentPolicy) -> Result<Self> {
        let prefix = raw.profile_prefix.trim();
        ensure!(
            !prefix.is_empty()
                && !prefix.starts_with('.')
                && !prefix.ends_with('.')
                && prefix
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-')),
            "profile_prefix must contain only ASCII letters, digits, dots, and hyphens"
        );
        ensure!(
            !raw.compatibility_capabilities.is_empty()
                && raw
                    .compatibility_capabilities
                    .iter()
                    .all(|name| !name.is_empty()
                        && name.bytes().all(|byte| byte.is_ascii_alphanumeric())),
            "at least one alphanumeric compatibility capability is required"
        );
        ensure!(
            raw.pipe.maximum_frame_bytes <= usize::try_from(raw.pipe.buffer_bytes)?,
            "maximum_frame_bytes cannot exceed pipe buffer_bytes"
        );
        ensure!(
            raw.workload.backpressure_payload_bytes < raw.pipe.maximum_frame_bytes,
            "backpressure_payload_bytes must leave room for protocol framing"
        );
        ensure!(
            raw.workload.launch_distribution_repetitions >= 2,
            "launch_distribution_repetitions must include a first observation and a repeat"
        );
        ensure!(
            raw.job.cpu_hard_cap_basis_points <= 10_000,
            "cpu_hard_cap_basis_points cannot exceed 10000"
        );
        ensure!(
            raw.workload.generation > 1,
            "generation must be greater than one so stale rejection can be exercised"
        );
        ensure!(
            raw.workload.storage_value_limit_bytes <= raw.workload.storage_quota_bytes,
            "storage_value_limit_bytes cannot exceed storage_quota_bytes"
        );
        Ok(Self {
            profile_prefix: prefix.to_owned(),
            compatibility_capabilities: raw.compatibility_capabilities.into_boxed_slice(),
            job: JobPolicy {
                active_process_limit: NonZeroU32::new(raw.job.active_process_limit)
                    .context("active_process_limit must be nonzero")?,
                memory_limit_bytes: NonZeroUsize::new(raw.job.memory_limit_bytes)
                    .context("memory_limit_bytes must be nonzero")?,
                cpu_hard_cap_basis_points: NonZeroU32::new(raw.job.cpu_hard_cap_basis_points)
                    .context("cpu_hard_cap_basis_points must be nonzero")?,
                kill_on_close: raw.job.kill_on_close,
                ui_restrictions: raw.job.ui_restrictions,
            },
            pipe: PipePolicy {
                buffer_bytes: NonZeroU32::new(raw.pipe.buffer_bytes)
                    .context("buffer_bytes must be nonzero")?,
                maximum_frame_bytes: NonZeroUsize::new(raw.pipe.maximum_frame_bytes)
                    .context("maximum_frame_bytes must be nonzero")?,
                connect_timeout: nonzero_duration(
                    raw.pipe.connect_timeout_ms,
                    "connect_timeout_ms",
                )?,
                operation_timeout: nonzero_duration(
                    raw.pipe.operation_timeout_ms,
                    "operation_timeout_ms",
                )?,
            },
            process: raw.process,
            http: raw.http.try_into()?,
            workload: raw.workload.try_into()?,
            faults: FaultPolicy {
                scenarios: nonempty_unique_scenarios(raw.faults.scenarios)?,
                allocation_chunk_bytes: NonZeroUsize::new(raw.faults.allocation_chunk_bytes)
                    .context("allocation_chunk_bytes must be nonzero")?,
                termination_exit_code: NonZeroU32::new(raw.faults.termination_exit_code)
                    .context("termination_exit_code must be nonzero")?,
            },
        })
    }
}

impl TryFrom<RawWorkloadPolicy> for WorkloadPolicy {
    type Error = anyhow::Error;

    fn try_from(raw: RawWorkloadPolicy) -> Result<Self> {
        Ok(Self {
            generation: ExtensionGeneration::new(raw.generation)?,
            echo_samples: NonZeroUsize::new(raw.echo_samples)
                .context("echo_samples must be nonzero")?,
            cohort_sizes: nonzero_sizes(raw.cohort_sizes, "cohort_sizes")?,
            launch_distribution_repetitions: NonZeroUsize::new(raw.launch_distribution_repetitions)
                .context("launch_distribution_repetitions must be nonzero")?,
            nested_job_context_timeout: nonzero_duration(
                raw.nested_job_context_timeout_ms,
                "nested_job_context_timeout_ms",
            )?,
            shared_host_contexts: NonZeroUsize::new(raw.shared_host_contexts)
                .context("shared_host_contexts must be nonzero")?,
            shared_host_noop_samples: NonZeroUsize::new(raw.shared_host_noop_samples)
                .context("shared_host_noop_samples must be nonzero")?,
            storage_key_limit_bytes: NonZeroUsize::new(raw.storage_key_limit_bytes)
                .context("storage_key_limit_bytes must be nonzero")?,
            storage_value_limit_bytes: NonZeroUsize::new(raw.storage_value_limit_bytes)
                .context("storage_value_limit_bytes must be nonzero")?,
            storage_entry_limit: NonZeroUsize::new(raw.storage_entry_limit)
                .context("storage_entry_limit must be nonzero")?,
            storage_quota_bytes: NonZeroUsize::new(raw.storage_quota_bytes)
                .context("storage_quota_bytes must be nonzero")?,
            responsiveness_samples: NonZeroUsize::new(raw.responsiveness_samples)
                .context("responsiveness_samples must be nonzero")?,
            backpressure_payload_bytes: NonZeroUsize::new(raw.backpressure_payload_bytes)
                .context("backpressure_payload_bytes must be nonzero")?,
            backpressure_attempt_limit: NonZeroUsize::new(raw.backpressure_attempt_limit)
                .context("backpressure_attempt_limit must be nonzero")?,
        })
    }
}

impl JobPolicy {
    pub(super) const fn active_process_limit(self) -> u32 {
        self.active_process_limit.get()
    }

    pub(super) const fn memory_limit_bytes(self) -> usize {
        self.memory_limit_bytes.get()
    }

    pub(super) const fn cpu_hard_cap_basis_points(self) -> u32 {
        self.cpu_hard_cap_basis_points.get()
    }

    pub(super) const fn kill_on_close(self) -> bool {
        self.kill_on_close
    }

    pub(super) const fn ui_restrictions(self) -> bool {
        self.ui_restrictions
    }
}

impl PipePolicy {
    pub(super) const fn buffer_bytes(self) -> u32 {
        self.buffer_bytes.get()
    }

    pub(super) const fn maximum_frame_bytes(self) -> usize {
        self.maximum_frame_bytes.get()
    }

    pub(super) const fn connect_timeout(self) -> Duration {
        self.connect_timeout
    }

    pub(super) const fn operation_timeout(self) -> Duration {
        self.operation_timeout
    }
}

impl ProcessPolicy {
    pub(super) const fn disable_win32k(self) -> bool {
        self.disable_win32k
    }

    pub(super) const fn restrict_child_processes(self) -> bool {
        self.restrict_child_processes
    }

    pub(super) const fn opt_out_all_application_packages(self) -> bool {
        self.opt_out_all_application_packages
    }
}

impl WorkloadPolicy {
    pub(super) const fn generation(&self) -> ExtensionGeneration {
        self.generation
    }

    pub(super) const fn echo_samples(&self) -> usize {
        self.echo_samples.get()
    }

    pub(super) fn cohort_sizes(&self) -> impl Iterator<Item = usize> + '_ {
        self.cohort_sizes.iter().map(|size| size.get())
    }

    pub(super) const fn launch_distribution_repetitions(&self) -> usize {
        self.launch_distribution_repetitions.get()
    }

    pub(super) const fn nested_job_context_timeout(&self) -> Duration {
        self.nested_job_context_timeout
    }

    pub(super) const fn shared_host_contexts(&self) -> usize {
        self.shared_host_contexts.get()
    }

    pub(super) const fn shared_host_noop_samples(&self) -> usize {
        self.shared_host_noop_samples.get()
    }

    pub(super) const fn storage_value_limit_bytes(&self) -> usize {
        self.storage_value_limit_bytes.get()
    }

    pub(super) const fn storage_key_limit_bytes(&self) -> usize {
        self.storage_key_limit_bytes.get()
    }

    pub(super) const fn storage_quota_bytes(&self) -> usize {
        self.storage_quota_bytes.get()
    }

    pub(super) const fn storage_entry_limit(&self) -> usize {
        self.storage_entry_limit.get()
    }

    pub(super) const fn responsiveness_samples(&self) -> usize {
        self.responsiveness_samples.get()
    }

    pub(super) const fn backpressure_payload_bytes(&self) -> usize {
        self.backpressure_payload_bytes.get()
    }

    pub(super) const fn backpressure_attempt_limit(&self) -> usize {
        self.backpressure_attempt_limit.get()
    }
}

impl FaultPolicy {
    pub(super) fn scenarios(&self) -> impl Iterator<Item = FaultScenario> + '_ {
        self.scenarios.iter().copied()
    }

    pub(super) const fn allocation_chunk_bytes(&self) -> usize {
        self.allocation_chunk_bytes.get()
    }

    pub(super) const fn termination_exit_code(&self) -> u32 {
        self.termination_exit_code.get()
    }
}

fn nonzero_duration(milliseconds: u32, field: &str) -> Result<Duration> {
    ensure!(milliseconds > 0, "{field} must be nonzero");
    Ok(Duration::from_millis(u64::from(milliseconds)))
}

fn nonzero_sizes(values: Vec<usize>, field: &str) -> Result<Box<[NonZeroUsize]>> {
    ensure!(!values.is_empty(), "{field} must not be empty");
    let values = values
        .into_iter()
        .map(|value| NonZeroUsize::new(value).with_context(|| format!("{field} contains zero")))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        values.windows(2).all(|pair| pair[0] < pair[1]),
        "{field} must be strictly increasing"
    );
    Ok(values.into_boxed_slice())
}

fn nonempty_unique_scenarios(values: Vec<FaultScenario>) -> Result<Box<[FaultScenario]>> {
    ensure!(!values.is_empty(), "fault scenarios must not be empty");
    ensure!(
        values
            .iter()
            .enumerate()
            .all(|(index, value)| !values[..index].contains(value)),
        "fault scenarios must be unique"
    );
    Ok(values.into_boxed_slice())
}

#[cfg(test)]
mod tests;
