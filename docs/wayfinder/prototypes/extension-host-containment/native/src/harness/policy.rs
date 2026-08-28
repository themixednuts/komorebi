use std::num::{NonZeroU32, NonZeroUsize};
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use crate::protocol::{ExtensionGeneration, FaultScenario};

#[derive(Debug, Clone)]
pub(super) struct ContainmentPolicy {
    profile_prefix: String,
    compatibility_capabilities: Box<[String]>,
    job: JobPolicy,
    pipe: PipePolicy,
    process: ProcessPolicy,
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
    shared_host_contexts: NonZeroUsize,
    shared_host_noop_samples: NonZeroUsize,
    storage_value_limit_bytes: NonZeroUsize,
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
    shared_host_contexts: usize,
    shared_host_noop_samples: usize,
    storage_value_limit_bytes: usize,
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
            raw.pipe.maximum_frame_bytes <= raw.pipe.buffer_bytes as usize,
            "maximum_frame_bytes cannot exceed pipe buffer_bytes"
        );
        ensure!(
            raw.workload.backpressure_payload_bytes < raw.pipe.maximum_frame_bytes,
            "backpressure_payload_bytes must leave room for protocol framing"
        );
        ensure!(
            raw.job.cpu_hard_cap_basis_points <= 10_000,
            "cpu_hard_cap_basis_points cannot exceed 10000"
        );
        ensure!(
            raw.workload.generation > 1,
            "generation must be greater than one so stale rejection can be exercised"
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
            workload: WorkloadPolicy {
                generation: ExtensionGeneration::new(raw.workload.generation)?,
                echo_samples: NonZeroUsize::new(raw.workload.echo_samples)
                    .context("echo_samples must be nonzero")?,
                cohort_sizes: nonzero_sizes(raw.workload.cohort_sizes, "cohort_sizes")?,
                shared_host_contexts: NonZeroUsize::new(raw.workload.shared_host_contexts)
                    .context("shared_host_contexts must be nonzero")?,
                shared_host_noop_samples: NonZeroUsize::new(raw.workload.shared_host_noop_samples)
                    .context("shared_host_noop_samples must be nonzero")?,
                storage_value_limit_bytes: NonZeroUsize::new(
                    raw.workload.storage_value_limit_bytes,
                )
                .context("storage_value_limit_bytes must be nonzero")?,
                backpressure_payload_bytes: NonZeroUsize::new(
                    raw.workload.backpressure_payload_bytes,
                )
                .context("backpressure_payload_bytes must be nonzero")?,
                backpressure_attempt_limit: NonZeroUsize::new(
                    raw.workload.backpressure_attempt_limit,
                )
                .context("backpressure_attempt_limit must be nonzero")?,
            },
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

    pub(super) const fn shared_host_contexts(&self) -> usize {
        self.shared_host_contexts.get()
    }

    pub(super) const fn shared_host_noop_samples(&self) -> usize {
        self.shared_host_noop_samples.get()
    }

    pub(super) const fn storage_value_limit_bytes(&self) -> usize {
        self.storage_value_limit_bytes.get()
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
mod tests {
    use crate::protocol::FaultScenario;

    use super::{ContainmentPolicy, RawContainmentPolicy};

    fn raw_policy() -> RawContainmentPolicy {
        serde_json::from_str(
            r#"{
                "profile_prefix":"komorebi.wayfinder",
                "compatibility_capabilities":["lpacAppExperience"],
                "job":{"active_process_limit":1,"memory_limit_bytes":1024,"cpu_hard_cap_basis_points":2000,"kill_on_close":true,"ui_restrictions":true},
                "pipe":{"buffer_bytes":65536,"maximum_frame_bytes":65536,"connect_timeout_ms":1000,"operation_timeout_ms":1000},
                "process":{"disable_win32k":true,"restrict_child_processes":true,"opt_out_all_application_packages":true},
                "workload":{"generation":2,"echo_samples":32,"cohort_sizes":[1,4,16],"shared_host_contexts":16,"shared_host_noop_samples":32,"storage_value_limit_bytes":262144,"backpressure_payload_bytes":49152,"backpressure_attempt_limit":4},
                "faults":{"scenarios":["cpu_loop","allocation_pressure","deadlock","indefinite_wait","pipe_stall","disconnect","lua_jit_native_crash"],"allocation_chunk_bytes":1048576,"termination_exit_code":57005}
            }"#,
        )
        .expect("valid policy fixture")
    }

    #[test]
    fn rejects_cpu_cap_above_one_hundred_percent() {
        let mut raw = raw_policy();
        raw.job.cpu_hard_cap_basis_points = 10_001;

        let error = ContainmentPolicy::try_from(raw).expect_err("reject invalid CPU cap");

        assert!(error.to_string().contains("cannot exceed 10000"));
    }

    #[test]
    fn rejects_frame_larger_than_pipe_buffer() {
        let mut raw = raw_policy();
        raw.pipe.maximum_frame_bytes = 65_537;

        let error = ContainmentPolicy::try_from(raw).expect_err("reject invalid frame limit");

        assert!(error.to_string().contains("cannot exceed pipe buffer"));
    }

    #[test]
    fn rejects_duplicate_fault_scenarios() {
        let mut raw = raw_policy();
        raw.faults.scenarios = vec![FaultScenario::CpuLoop, FaultScenario::CpuLoop];

        let error = ContainmentPolicy::try_from(raw).expect_err("reject duplicate fault scenario");

        assert!(error.to_string().contains("must be unique"));
    }
}
