use std::fs;
use std::path::Path;
use std::time::Instant;

use anyhow::{Result, ensure};

use crate::protocol::{ChildFrame, FaultScenario, HostFrame, RuntimeKind};

use super::ipc::ReceiveError;
use super::launch::{AuthenticatedExtension, ExtensionBehavior, launch};
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
        .map(|scenario| {
            Ok(arm(scenario, executable, private_file, policy)?
                .observe(policy)?
                .evidence)
        })
        .collect()
}

pub(super) struct ArmedFault {
    extension: AuthenticatedExtension,
    scenario: FaultScenario,
    pub(super) armed_at: Instant,
}

pub(super) struct ObservedFault {
    pub(super) evidence: FaultEvidence,
    pub(super) armed_at: Instant,
    pub(super) observed_at: Instant,
}

pub(super) fn arm(
    scenario: FaultScenario,
    executable: &Path,
    private_file: &Path,
    policy: &ContainmentPolicy,
) -> Result<ArmedFault> {
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
    Ok(ArmedFault {
        extension,
        scenario,
        armed_at: Instant::now(),
    })
}

impl ArmedFault {
    pub(super) fn observe(mut self, policy: &ContainmentPolicy) -> Result<ObservedFault> {
        let started = self.armed_at;
        let mut observation = match self
            .extension
            .channel
            .receive(policy.pipe().operation_timeout())
        {
            Err(ReceiveError::Deadline) => FaultIpcObservation::Deadline,
            Err(_) => FaultIpcObservation::Disconnected,
            Ok(_) => FaultIpcObservation::UnexpectedFrame,
        };
        let observed_at = Instant::now();
        let trigger_to_observation_ms = observed_at.duration_since(started).as_secs_f64() * 1_000.0;
        let immediate_exit = self.extension.wait_for_exit(std::time::Duration::ZERO)?;
        if matches!(observation, FaultIpcObservation::Deadline) && immediate_exit.is_some() {
            observation = FaultIpcObservation::Disconnected;
        }
        let mut exit_code = if immediate_exit.is_some() {
            immediate_exit
        } else if matches!(observation, FaultIpcObservation::Deadline) {
            None
        } else {
            self.extension
                .wait_for_exit(policy.pipe().operation_timeout())?
        };
        let termination_started = Instant::now();
        let forced_tree_termination = exit_code.is_none();
        if forced_tree_termination {
            self.extension
                .terminate_tree(policy.faults().termination_exit_code())?;
            exit_code = self
                .extension
                .wait_for_exit(policy.pipe().operation_timeout())?;
        }
        let termination_to_exit_ms =
            forced_tree_termination.then(|| termination_started.elapsed().as_secs_f64() * 1_000.0);
        let exit_code =
            exit_code.ok_or_else(|| anyhow::anyhow!("fault process tree did not terminate"))?;
        let diagnostic = fs::read_to_string(&self.extension.error_file)
            .unwrap_or_else(|error| format!("unable to read fault diagnostic: {error}"));
        ensure!(
            self.scenario != FaultScenario::LuaJitNativeCrash || exit_code != 1,
            "LuaJIT fault returned a Rust error instead of crashing: {diagnostic}"
        );
        verify_expected(self.scenario, observation, forced_tree_termination)?;
        Ok(ObservedFault {
            evidence: FaultEvidence {
                scenario: self.scenario,
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
            },
            armed_at: self.armed_at,
            observed_at,
        })
    }
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
