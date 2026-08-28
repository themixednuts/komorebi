use std::num::NonZeroU64;
use std::path::Path;
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, ensure};

use crate::protocol::FaultScenario;

use super::fault::{ObservedFault, arm};
use super::percentile;
use super::policy::ContainmentPolicy;
use super::report::{HostResponsivenessEvidence, Verification};

#[derive(Clone, Copy)]
struct ManagerRevision(NonZeroU64);

impl ManagerRevision {
    const INITIAL: Self = Self(NonZeroU64::MIN);

    fn advance(self) -> Result<Self> {
        self.0
            .checked_add(1)
            .map(Self)
            .context("manager revision overflow")
    }
}

struct ManagerCommand {
    sequence: u64,
    requested_at: Instant,
    reply: SyncSender<ManagerSettlement>,
}

#[derive(Clone, Copy)]
struct ManagerSettlement {
    sequence: u64,
    revision: ManagerRevision,
    requested_at: Instant,
}

#[derive(Clone, Copy)]
struct CommandRoundTrip {
    requested_at: Instant,
    acknowledged_at: Instant,
}

pub(super) fn run(
    executable: &Path,
    private_file: &Path,
    policy: &ContainmentPolicy,
) -> Result<HostResponsivenessEvidence> {
    let fault_policy = policy.clone();
    let fault_executable = executable.to_path_buf();
    let fault_private_file = private_file.to_path_buf();
    let (armed_sender, armed_receiver) = sync_channel(0);
    let fault_worker = std::thread::Builder::new()
        .name("extension-fault-supervisor".to_owned())
        .spawn(move || -> Result<ObservedFault> {
            let armed = arm(
                FaultScenario::CpuLoop,
                &fault_executable,
                &fault_private_file,
                &fault_policy,
            )?;
            armed_sender
                .send(armed.armed_at)
                .context("publish armed fault window")?;
            armed.observe(&fault_policy)
        })
        .context("spawn extension fault supervisor")?;

    let armed_at = match armed_receiver.recv_timeout(policy.pipe().connect_timeout()) {
        Ok(armed_at) => armed_at,
        Err(error) => {
            return match join_worker(fault_worker, "extension fault supervisor") {
                Err(fault_error) => Err(fault_error).context("fault failed before arming"),
                Ok(_) => Err(anyhow!(error)).context("wait for armed fault window"),
            };
        }
    };

    let sample_count = policy.workload().responsiveness_samples();
    let command_timeout = policy.pipe().operation_timeout();
    let (command_sender, command_receiver) = sync_channel(0);
    let requester = match std::thread::Builder::new()
        .name("manager-command-requester".to_owned())
        .spawn(move || request_commands(&command_sender, sample_count, command_timeout))
    {
        Ok(requester) => requester,
        Err(error) => {
            let fault_result = join_worker(fault_worker, "extension fault supervisor");
            return match fault_result {
                Ok(_) => Err(error).context("spawn manager command requester"),
                Err(fault_error) => Err(fault_error)
                    .context("fault supervisor also failed after requester spawn failure"),
            };
        }
    };

    let owner_result = settle_commands(&command_receiver, sample_count, command_timeout);
    drop(command_receiver);
    let requester_result = join_worker(requester, "manager command requester");
    let fault_result = join_worker(fault_worker, "extension fault supervisor");
    let revision = owner_result?;
    let round_trips = requester_result?;
    let observed = fault_result?;

    ensure!(
        round_trips.len() == sample_count,
        "manager command requester returned an incomplete sample set"
    );
    let all_inside_fault_window = round_trips.iter().all(|sample| {
        sample.requested_at >= armed_at && sample.acknowledged_at <= observed.observed_at
    });
    ensure!(
        all_inside_fault_window,
        "a manager command settled outside the armed fault window"
    );

    let mut roundtrip_us: Vec<_> = round_trips
        .iter()
        .map(|sample| {
            sample
                .acknowledged_at
                .duration_since(sample.requested_at)
                .as_secs_f64()
                * 1_000_000.0
        })
        .collect();
    roundtrip_us.sort_by(f64::total_cmp);
    let maximum_roundtrip_us = roundtrip_us
        .last()
        .copied()
        .context("missing latency sample")?;

    Ok(HostResponsivenessEvidence {
        scenario: FaultScenario::CpuLoop,
        manager_owner: "harness main thread",
        fault_supervision: "dedicated extension-supervision thread",
        synchronization: "blocking Rust channels plus overlapped named-pipe kernel event",
        command_samples: sample_count,
        final_manager_revision: revision.0.get(),
        commands_settled_within_fault_window: Verification::Passed,
        action_roundtrip_p50_us: percentile(&roundtrip_us, 50, 100),
        action_roundtrip_p99_us: percentile(&roundtrip_us, 99, 100),
        action_roundtrip_max_us: maximum_roundtrip_us,
        fault_window_ms: observed
            .observed_at
            .duration_since(observed.armed_at)
            .as_secs_f64()
            * 1_000.0,
        fault_process_tree_terminated: observed.evidence.process_tree_terminated,
        fault_exit_code: observed.evidence.exit_code,
    })
}

fn request_commands(
    command_sender: &SyncSender<ManagerCommand>,
    sample_count: usize,
    timeout: Duration,
) -> Result<Vec<CommandRoundTrip>> {
    let mut round_trips = Vec::with_capacity(sample_count);
    let mut last_revision = ManagerRevision::INITIAL;
    for index in 0..sample_count {
        let sequence = u64::try_from(index)?;
        let requested_at = Instant::now();
        let (reply, settlement) = sync_channel(0);
        command_sender
            .send(ManagerCommand {
                sequence,
                requested_at,
                reply,
            })
            .context("submit manager responsiveness command")?;
        let response = settlement
            .recv_timeout(timeout)
            .context("wait for manager responsiveness settlement")?;
        ensure!(
            response.sequence == sequence,
            "manager settled wrong sequence"
        );
        ensure!(
            response.requested_at == requested_at,
            "manager settlement changed request identity"
        );
        ensure!(
            response.revision.0 > last_revision.0,
            "manager revision did not advance"
        );
        last_revision = response.revision;
        round_trips.push(CommandRoundTrip {
            requested_at,
            acknowledged_at: Instant::now(),
        });
    }
    Ok(round_trips)
}

fn settle_commands(
    command_receiver: &Receiver<ManagerCommand>,
    sample_count: usize,
    timeout: Duration,
) -> Result<ManagerRevision> {
    let mut revision = ManagerRevision::INITIAL;
    for index in 0..sample_count {
        let command = command_receiver
            .recv_timeout(timeout)
            .context("wait for manager responsiveness command")?;
        ensure!(
            command.sequence == u64::try_from(index)?,
            "manager received an out-of-order command"
        );
        revision = revision.advance()?;
        command
            .reply
            .send(ManagerSettlement {
                sequence: command.sequence,
                revision,
                requested_at: command.requested_at,
            })
            .context("settle manager responsiveness command")?;
    }
    Ok(revision)
}

fn join_worker<T>(worker: JoinHandle<Result<T>>, name: &str) -> Result<T> {
    worker
        .join()
        .map_err(|_| anyhow!("{name} panicked"))?
        .with_context(|| format!("{name} failed"))
}
