use std::fs;
use std::path::Path;
use std::time::Instant;

use anyhow::{Result, ensure};

use crate::protocol::{ChildFrame, FaultScenario, HostFrame, RuntimeKind};

use super::ipc::ReceiveError;
use super::launch::{ExtensionBehavior, launch};
use super::policy::ContainmentPolicy;
use super::report::{FaultEvidence, FaultIpcObservation, TerminationMode, Verification};

pub(super) fn run(
    executable: &Path,
    private_file: &Path,
    policy: &ContainmentPolicy,
) -> Result<Vec<FaultEvidence>> {
    policy
        .faults()
        .scenarios()
        .map(|scenario| run_one(scenario, executable, private_file, policy))
        .collect()
}

fn run_one(
    scenario: FaultScenario,
    executable: &Path,
    private_file: &Path,
    policy: &ContainmentPolicy,
) -> Result<FaultEvidence> {
    let mut extension = launch(
        RuntimeKind::Rust,
        executable,
        private_file,
        policy,
        ExtensionBehavior::Fault(scenario),
        policy.workload().generation(),
    )?;
    extension.channel.send(&HostFrame::RunFault {
        generation: policy.workload().generation(),
    })?;
    let armed = extension
        .channel
        .receive(policy.pipe().operation_timeout())?;
    ensure!(
        matches!(armed, ChildFrame::FaultArmed { generation, scenario: armed_scenario }
            if generation == policy.workload().generation() && armed_scenario == scenario),
        "fault child did not arm the requested scenario"
    );
    let started = Instant::now();
    let mut observation = match extension.channel.receive(policy.pipe().operation_timeout()) {
        Err(ReceiveError::Deadline) => FaultIpcObservation::Deadline,
        Err(_) => FaultIpcObservation::Disconnected,
        Ok(_) => FaultIpcObservation::UnexpectedFrame,
    };
    let trigger_to_observation_ms = started.elapsed().as_secs_f64() * 1_000.0;
    let immediate_exit = extension.wait_for_exit(std::time::Duration::ZERO)?;
    if matches!(observation, FaultIpcObservation::Deadline) && immediate_exit.is_some() {
        observation = FaultIpcObservation::Disconnected;
    }
    let mut exit_code = if immediate_exit.is_some() {
        immediate_exit
    } else if matches!(observation, FaultIpcObservation::Deadline) {
        None
    } else {
        extension.wait_for_exit(policy.pipe().operation_timeout())?
    };
    let termination_started = Instant::now();
    let forced_tree_termination = exit_code.is_none();
    if forced_tree_termination {
        extension.terminate_tree(policy.faults().termination_exit_code())?;
        exit_code = extension.wait_for_exit(policy.pipe().operation_timeout())?;
    }
    let termination_to_exit_ms =
        forced_tree_termination.then(|| termination_started.elapsed().as_secs_f64() * 1_000.0);
    let exit_code =
        exit_code.ok_or_else(|| anyhow::anyhow!("fault process tree did not terminate"))?;
    let diagnostic = fs::read_to_string(&extension.error_file)
        .unwrap_or_else(|error| format!("unable to read fault diagnostic: {error}"));
    ensure!(
        scenario != FaultScenario::LuaJitNativeCrash || exit_code != 1,
        "LuaJIT fault returned a Rust error instead of crashing: {diagnostic}"
    );
    verify_expected(scenario, observation, forced_tree_termination)?;
    Ok(FaultEvidence {
        scenario,
        ipc_observation: observation,
        termination_mode: if forced_tree_termination {
            TerminationMode::ForcedJob
        } else {
            TerminationMode::Natural
        },
        process_tree_terminated: Verification::Passed,
        trigger_to_observation_ms,
        termination_to_exit_ms,
        exit_code,
    })
}

fn verify_expected(
    scenario: FaultScenario,
    observation: FaultIpcObservation,
    forced: bool,
) -> Result<()> {
    let blocks = matches!(
        scenario,
        FaultScenario::CpuLoop
            | FaultScenario::Deadlock
            | FaultScenario::IndefiniteWait
            | FaultScenario::PipeStall
    );
    if blocks {
        ensure!(
            matches!(observation, FaultIpcObservation::Deadline) && forced,
            "blocking fault {scenario:?} observed {observation:?} with forced={forced}"
        );
    } else {
        ensure!(
            matches!(observation, FaultIpcObservation::Disconnected) && !forced,
            "terminating fault {scenario:?} observed {observation:?} with forced={forced}"
        );
    }
    Ok(())
}
