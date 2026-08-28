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
            "http":{"allowed_hosts":["example.com"],"allowed_media_types":["text/html"],"maximum_redirects":3,"maximum_response_bytes":1048576,"maximum_total_bytes":2097152,"maximum_requests":8,"maximum_response_header_bytes":32768,"timeout_ms":5000},
            "workload":{"generation":2,"echo_samples":32,"cohort_sizes":[1,4,16],"launch_distribution_repetitions":5,"nested_job_context_timeout_ms":30000,"shared_host_contexts":16,"shared_host_noop_samples":32,"storage_key_limit_bytes":128,"storage_value_limit_bytes":262144,"storage_entry_limit":256,"storage_quota_bytes":393216,"responsiveness_samples":64,"backpressure_payload_bytes":49152,"backpressure_attempt_limit":4},
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

#[test]
fn rejects_zero_nested_job_context_timeout() {
    let mut raw = raw_policy();
    raw.workload.nested_job_context_timeout_ms = 0;
    let error = ContainmentPolicy::try_from(raw).expect_err("reject zero helper timeout");
    assert!(error.to_string().contains("must be nonzero"));
}
