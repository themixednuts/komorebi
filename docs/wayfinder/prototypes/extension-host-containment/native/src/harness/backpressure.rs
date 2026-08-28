use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Result, ensure};

use crate::protocol::{ChildFrame, FaultScenario, HostFrame, RuntimeKind};

use super::ipc::SendError;
use super::launch::{ExtensionBehavior, launch};
use super::policy::ContainmentPolicy;
use super::report::{BackpressureEvidence, Verification};

pub(super) fn run(
    executable: &Path,
    private_file: &Path,
    policy: &ContainmentPolicy,
) -> Result<BackpressureEvidence> {
    let generation = policy.workload().generation();
    let mut extension = launch(
        RuntimeKind::Rust,
        executable,
        private_file,
        policy,
        ExtensionBehavior::Fault(FaultScenario::PipeStall),
        generation,
    )?;
    extension
        .channel
        .send(&HostFrame::RunFault { generation })?;
    let armed = extension
        .channel
        .receive(policy.pipe().operation_timeout())?;
    ensure!(
        matches!(armed, ChildFrame::FaultArmed {
            generation: armed_generation,
            scenario: FaultScenario::PipeStall,
        } if armed_generation == generation),
        "backpressure child did not arm its no-read state"
    );

    let workload = policy.workload();
    let payload_bytes = workload.backpressure_payload_bytes();
    let payload = "x".repeat(payload_bytes);
    let attempt_limit = workload.backpressure_attempt_limit();
    let mut completed_writes = 0_usize;
    let mut blocked_write_cancel_ms = None;
    for attempt in 0..attempt_limit {
        let write_started = Instant::now();
        let sequence = u64::try_from(attempt)?;
        match extension.channel.send(&HostFrame::BackpressureChunk {
            generation,
            sequence,
            payload: payload.clone(),
        }) {
            Ok(()) => {
                completed_writes = completed_writes
                    .checked_add(1)
                    .ok_or_else(|| anyhow::anyhow!("completed-write count overflow"))?;
            }
            Err(SendError::Deadline) => {
                blocked_write_cancel_ms = Some(write_started.elapsed().as_secs_f64() * 1_000.0);
                break;
            }
            Err(error) => return Err(error.into()),
        }
    }
    ensure!(completed_writes > 0, "pipe accepted no bounded writes");
    let blocked_write_cancel_ms = blocked_write_cancel_ms
        .ok_or_else(|| anyhow::anyhow!("pipe did not apply backpressure within attempt limit"))?;
    ensure!(
        extension.wait_for_exit(Duration::ZERO)?.is_none(),
        "backpressure child exited before Job termination"
    );
    extension.terminate_tree(policy.faults().termination_exit_code())?;
    let exit_code = extension
        .wait_for_exit(policy.pipe().operation_timeout())?
        .ok_or_else(|| anyhow::anyhow!("backpressure child did not terminate"))?;

    Ok(BackpressureEvidence {
        transport: "authenticated named pipe",
        payload_bytes,
        attempt_limit,
        completed_writes,
        completed_payload_bytes: completed_writes
            .checked_mul(payload_bytes)
            .ok_or_else(|| anyhow::anyhow!("completed-payload byte count overflow"))?,
        blocked_write_cancel_ms,
        blocked_write_cancelled: Verification::Passed,
        process_tree_terminated: Verification::Passed,
        exit_code,
    })
}
