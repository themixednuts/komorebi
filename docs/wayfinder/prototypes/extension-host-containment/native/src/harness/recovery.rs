use std::path::Path;
use std::time::Instant;

use anyhow::{Context, Result, ensure};
use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
use windows_sys::Win32::System::Threading::WaitForSingleObject;

use crate::protocol::{ExtensionGeneration, RuntimeKind};

use super::launch::{AuthenticatedExtension, ExtensionBehavior, launch};
use super::policy::ContainmentPolicy;
use super::report::{RestartRecoveryEvidence, Verification};
use super::session::serve;

struct RestartPermit(ExtensionGeneration);

impl RestartPermit {
    fn into_generation(self) -> ExtensionGeneration {
        self.0
    }
}

struct OneRestartBudget(Option<RestartPermit>);

impl OneRestartBudget {
    fn after(initial_generation: ExtensionGeneration) -> Result<Self> {
        Ok(Self(Some(RestartPermit(initial_generation.next()?))))
    }

    fn claim(&mut self) -> Option<RestartPermit> {
        self.0.take()
    }
}

fn launch_replacement(
    permit: RestartPermit,
    executable: &Path,
    private_file: &Path,
    policy: &ContainmentPolicy,
) -> Result<(AuthenticatedExtension, ExtensionGeneration)> {
    let generation = permit.into_generation();
    let extension = launch(
        RuntimeKind::Rust,
        executable,
        private_file,
        policy,
        ExtensionBehavior::Normal,
        generation,
    )?;
    Ok((extension, generation))
}

pub(super) fn run(
    executable: &Path,
    private_file: &Path,
    policy: &ContainmentPolicy,
) -> Result<RestartRecoveryEvidence> {
    let initial_generation = policy.workload().generation();
    let mut budget = OneRestartBudget::after(initial_generation)?;
    let initial = launch(
        RuntimeKind::Rust,
        executable,
        private_file,
        policy,
        ExtensionBehavior::Normal,
        initial_generation,
    )?;
    initial.terminate_tree(policy.faults().termination_exit_code())?;
    let initial_exit_code = initial
        .wait_for_exit(policy.pipe().operation_timeout())?
        .context("initial extension did not terminate")?;
    drop(initial);

    let recovery_started = Instant::now();
    let permit = budget
        .claim()
        .context("one-restart budget was unavailable")?;
    let (mut replacement, replacement_generation) =
        launch_replacement(permit, executable, private_file, policy)?;
    let session = serve(
        &mut replacement.channel,
        replacement_generation,
        replacement.process.raw(),
        &replacement.error_file,
        policy,
    )?;
    // SAFETY: replacement process remains owned and timeout is bounded by validated policy.
    let replacement_exited = unsafe {
        WaitForSingleObject(
            replacement.process.raw(),
            u32::try_from(policy.pipe().operation_timeout().as_millis())?,
        )
    } == WAIT_OBJECT_0;
    ensure!(replacement_exited, "replacement extension did not exit");
    let second_restart_denied = budget.claim().is_none();
    ensure!(
        second_restart_denied,
        "one-restart budget allowed a second restart"
    );

    Ok(RestartRecoveryEvidence {
        initial_generation: initial_generation.get(),
        replacement_generation: replacement_generation.get(),
        initial_exit_code,
        recovery_ms: recovery_started.elapsed().as_secs_f64() * 1_000.0,
        replacement_authenticated: Verification::Passed,
        replacement_session_completed: Verification::Passed,
        stale_generation_rejected: session.stale_generation_rejected,
        second_restart_denied: Verification::from(second_restart_denied),
    })
}

#[cfg(test)]
mod tests {
    use crate::protocol::ExtensionGeneration;

    use super::OneRestartBudget;

    #[test]
    fn one_restart_budget_issues_exactly_one_permit() {
        let initial = ExtensionGeneration::new(7).expect("valid initial generation");
        let mut budget = OneRestartBudget::after(initial).expect("valid restart budget");
        assert_eq!(
            budget
                .claim()
                .expect("first permit should be available")
                .0
                .get(),
            8
        );
        assert!(budget.claim().is_none());
    }
}
