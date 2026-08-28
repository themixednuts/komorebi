use std::path::Path;

use serde::Serialize;

use crate::protocol::{ChildFacts, FaultScenario, ParentExitMode, ProbeOutcome, RuntimeKind};
use crate::windows::windows_string_evidence;

#[derive(Debug, Serialize)]
pub(super) struct HarnessReport {
    pub(super) generated_at_unix_ms: u64,
    pub(super) platform: PlatformEvidence,
    pub(super) toolchain: ToolchainEvidence,
    pub(super) invocation: InvocationEvidence,
    pub(super) binaries: Vec<BinaryEvidence>,
    pub(super) boundary: BoundaryEvidence,
    pub(super) runs: Vec<RunReport>,
    pub(super) af_unix: AfUnixEvidence,
    pub(super) faults: Vec<FaultEvidence>,
    pub(super) host_responsiveness: HostResponsivenessEvidence,
    pub(super) backpressure: BackpressureEvidence,
    pub(super) parent_lifetime: Vec<ParentLifetimeEvidence>,
    pub(super) restart_recovery: RestartRecoveryEvidence,
    pub(super) scale: Vec<ScaleReport>,
    pub(super) shared_host_control: SharedHostControl,
    pub(super) cleanup: CleanupEvidence,
}

#[derive(Debug, Serialize)]
pub(super) struct AfUnixEvidence {
    pub(super) role: &'static str,
    pub(super) endpoint_encoding: &'static str,
    pub(super) samples: usize,
    pub(super) full_trust_process_echo_p99_us: f64,
    pub(super) child_exit_code: u32,
    pub(super) endpoint_cleanup: Verification,
    pub(super) lpac_socket_creation_denied: Verification,
    pub(super) kernel_peer_pid: &'static str,
    pub(super) kernel_peer_token: &'static str,
}

#[derive(Debug, Serialize)]
pub(super) struct BackpressureEvidence {
    pub(super) transport: &'static str,
    pub(super) payload_bytes: usize,
    pub(super) attempt_limit: usize,
    pub(super) completed_writes: usize,
    pub(super) completed_payload_bytes: usize,
    pub(super) blocked_write_cancel_ms: f64,
    pub(super) blocked_write_cancelled: Verification,
    pub(super) process_tree_terminated: Verification,
    pub(super) exit_code: u32,
}

#[derive(Debug, Serialize)]
pub(super) struct RestartRecoveryEvidence {
    pub(super) initial_generation: u64,
    pub(super) replacement_generation: u64,
    pub(super) initial_exit_code: u32,
    pub(super) recovery_ms: f64,
    pub(super) replacement_authenticated: Verification,
    pub(super) replacement_session_completed: Verification,
    pub(super) stale_generation_rejected: Verification,
    pub(super) second_restart_denied: Verification,
}

#[derive(Debug, Serialize)]
pub(super) struct ParentLifetimeEvidence {
    pub(super) mode: ParentExitMode,
    pub(super) child_workload: &'static str,
    pub(super) parent_exit_code: u32,
    pub(super) child_exit_code: u32,
    pub(super) child_exit_after_parent_ms: f64,
    pub(super) process_tree_terminated: Verification,
    pub(super) profiles_deleted: Verification,
}

#[derive(Debug, Serialize)]
pub(super) struct FaultEvidence {
    pub(super) scenario: FaultScenario,
    pub(super) ipc_observation: FaultIpcObservation,
    pub(super) termination_mode: TerminationMode,
    pub(super) process_tree_terminated: Verification,
    pub(super) trigger_to_observation_ms: f64,
    pub(super) termination_to_exit_ms: Option<f64>,
    pub(super) exit_code: u32,
}

#[derive(Debug, Serialize)]
pub(super) struct HostResponsivenessEvidence {
    pub(super) scenario: FaultScenario,
    pub(super) manager_owner: &'static str,
    pub(super) fault_supervision: &'static str,
    pub(super) synchronization: &'static str,
    pub(super) command_samples: usize,
    pub(super) final_manager_revision: u64,
    pub(super) commands_settled_within_fault_window: Verification,
    pub(super) action_roundtrip_p50_us: f64,
    pub(super) action_roundtrip_p99_us: f64,
    pub(super) action_roundtrip_max_us: f64,
    pub(super) fault_window_ms: f64,
    pub(super) fault_process_tree_terminated: Verification,
    pub(super) fault_exit_code: u32,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum FaultIpcObservation {
    Deadline,
    Disconnected,
    UnexpectedFrame,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum TerminationMode {
    Natural,
    ForcedJob,
}

#[derive(Debug, Serialize)]
pub(super) struct PlatformEvidence {
    pub(super) windows_major: u32,
    pub(super) windows_minor: u32,
    pub(super) windows_build: u32,
    pub(super) architecture: &'static str,
}

#[derive(Debug, Serialize)]
pub(super) struct ToolchainEvidence {
    pub(super) rustc_verbose_version: String,
    pub(super) cargo_version: String,
    pub(super) cargo_dependency_tree: String,
}

#[derive(Debug, Serialize)]
pub(super) struct InvocationEvidence {
    pub(super) working_directory: WindowsPathEvidence,
    pub(super) command: &'static str,
    pub(super) rustflags: &'static str,
}

#[derive(Debug, Serialize)]
pub(super) struct BinaryEvidence {
    pub(super) role: &'static str,
    pub(super) path: WindowsPathEvidence,
    pub(super) bytes: u64,
    pub(super) sha256: String,
    pub(super) pe_imports: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(super) struct WindowsPathEvidence {
    pub(super) utf8: Option<String>,
    pub(super) utf16_code_units_hex: String,
}

impl From<&Path> for WindowsPathEvidence {
    fn from(path: &Path) -> Self {
        let value = windows_string_evidence(path.as_os_str());
        Self {
            utf8: value.utf8,
            utf16_code_units_hex: value.utf16_code_units_hex,
        }
    }
}

#[derive(Debug, Serialize)]
pub(super) struct BoundaryEvidence {
    pub(super) security_identity: &'static str,
    pub(super) compatibility_capabilities: Vec<String>,
    pub(super) all_application_packages_policy: u32,
    pub(super) process_mitigation_policy: [u64; 2],
    pub(super) child_process_policy: u32,
    pub(super) resource_lifetime: &'static str,
    pub(super) job_active_process_limit: u32,
    pub(super) job_memory_limit_bytes: usize,
    pub(super) job_cpu_hard_cap_basis_points: u32,
    pub(super) ipc: &'static str,
    pub(super) pipe_flags: u32,
    pub(super) pipe_buffer_bytes: u32,
    pub(super) maximum_frame_bytes: usize,
    pub(super) dll_search: &'static str,
    pub(super) experimental_api_used: bool,
    pub(super) inherit_handles: bool,
    pub(super) create_no_window: bool,
}

#[derive(Debug, Serialize)]
pub(super) struct RunReport {
    pub(super) runtime: RuntimeKind,
    pub(super) profile_name: String,
    pub(super) expected_pid: u32,
    pub(super) pipe_reported_pid: u32,
    pub(super) pipe_acl_sddl: String,
    pub(super) foreign_profile_sid: String,
    pub(super) reparse_link_created: Verification,
    pub(super) startup_ms: f64,
    pub(super) private_commit_bytes: usize,
    pub(super) in_expected_job: Verification,
    pub(super) facts: ChildFacts,
    pub(super) probes: Vec<ProbeOutcome>,
    pub(super) echo_rtt_us: Vec<f64>,
    pub(super) broker_service_us: Vec<f64>,
    pub(super) storage_cas_roundtrip: Verification,
    pub(super) brokered_http_status: Option<u16>,
    pub(super) stale_generation_rejected: Verification,
    pub(super) exit_observed: Verification,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Verification {
    Passed,
    Failed,
}

impl From<bool> for Verification {
    fn from(value: bool) -> Self {
        if value { Self::Passed } else { Self::Failed }
    }
}

impl Verification {
    pub(super) const fn passed(self) -> bool {
        matches!(self, Self::Passed)
    }
}

#[derive(Debug, Serialize)]
pub(super) struct CleanupEvidence {
    pub(super) profiles_deleted: bool,
    pub(super) private_file_deleted: bool,
    pub(super) pipe_handles_closed: bool,
}

#[derive(Debug, Serialize)]
pub(super) struct ScaleReport {
    pub(super) process_count: usize,
    pub(super) cohort_wall_ms: f64,
    pub(super) authenticated_ready_p50_ms: f64,
    pub(super) authenticated_ready_p99_ms: f64,
    pub(super) aggregate_private_commit_bytes: usize,
    pub(super) echo_rtt_p99_us: f64,
    pub(super) forbidden_probes_allowed: usize,
    pub(super) all_exited: bool,
}

#[derive(Debug, Serialize)]
pub(super) struct SharedHostControl {
    pub(super) lua_contexts: usize,
    pub(super) cohort_startup_ms: f64,
    pub(super) incremental_private_commit_bytes: usize,
    pub(super) in_process_noop_p99_us: f64,
    pub(super) blast_radius_extensions: usize,
    pub(super) isolation_boundary: &'static str,
}
