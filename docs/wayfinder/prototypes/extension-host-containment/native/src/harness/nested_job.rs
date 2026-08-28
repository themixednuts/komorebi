use std::io::{Read, Write};
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::ptr::null;
use std::time::Duration;

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use windows_sys::Win32::Foundation::{WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_BREAKAWAY_OK,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK,
    JOB_OBJECT_UILIMIT_HANDLES, JOBOBJECT_BASIC_UI_RESTRICTIONS,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectBasicUIRestrictions,
    JobObjectExtendedLimitInformation, TerminateJobObject,
};
use windows_sys::Win32::System::Threading::{CREATE_NO_WINDOW, WaitForSingleObject};

use crate::protocol::{ExtensionWorkload, RuntimeKind};
use crate::windows::OwnedHandle;

use super::job_context::{HostJobMode, JobContextError, JobContextRejection, LaunchJobContext};
use super::policy::ContainmentPolicy;
use super::report::Verification;

const START: u8 = 0xA5;
const HELPER_TIMEOUT_EXIT_CODE: u32 = 0xDEAD_1001;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum OuterJobContext {
    NestedWithoutUiRestrictions,
    ExplicitBreakawayWithUiRestrictions,
    SilentBreakawayWithUiRestrictions,
    UiRestrictedWithoutBreakaway,
}

impl OuterJobContext {
    const ALL: [Self; 4] = [
        Self::NestedWithoutUiRestrictions,
        Self::ExplicitBreakawayWithUiRestrictions,
        Self::SilentBreakawayWithUiRestrictions,
        Self::UiRestrictedWithoutBreakaway,
    ];

    const fn limit_flags(self) -> u32 {
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
            | match self {
                Self::ExplicitBreakawayWithUiRestrictions => JOB_OBJECT_LIMIT_BREAKAWAY_OK,
                Self::SilentBreakawayWithUiRestrictions => JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK,
                Self::NestedWithoutUiRestrictions | Self::UiRestrictedWithoutBreakaway => 0,
            }
    }

    const fn has_ui_restrictions(self) -> bool {
        !matches!(self, Self::NestedWithoutUiRestrictions)
    }
}

#[derive(Debug, Serialize)]
pub(super) struct NestedJobEvidence {
    pub(super) active_host_mode: HostJobMode,
    pub(super) contexts: Vec<NestedJobContextEvidence>,
}

#[derive(Debug, Serialize)]
pub(super) struct NestedJobContextEvidence {
    pub(super) outer_context: OuterJobContext,
    pub(super) outcome: VerifiedNestedJobOutcome,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum VerifiedNestedJobOutcome {
    ExtensionLaunched {
        observed_mode: HostJobMode,
        in_inner_job: Verification,
        extension_exited: Verification,
    },
    LaunchRejected {
        reason: JobContextRejection,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum HelperOutcome {
    ExtensionLaunched {
        observed_mode: HostJobMode,
        in_inner_job: bool,
        extension_exited: bool,
    },
    LaunchRejected {
        reason: JobContextRejection,
    },
    Failed {
        message: String,
    },
}

pub(super) fn run_suite(
    host_executable: &Path,
    extension_executable: &Path,
    private_file: &Path,
    policy: &ContainmentPolicy,
) -> Result<NestedJobEvidence> {
    let active_context = LaunchJobContext::detect()?;
    let contexts = OuterJobContext::ALL
        .into_iter()
        .map(|outer_context| {
            run_context(
                active_context,
                outer_context,
                host_executable,
                extension_executable,
                private_file,
                policy,
            )
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(NestedJobEvidence {
        active_host_mode: active_context.mode(),
        contexts,
    })
}

pub(super) fn run_parent(
    extension_executable: &Path,
    private_file: &Path,
    policy: &ContainmentPolicy,
) -> Result<()> {
    await_start()?;
    let outcome = match super::run_extension(
        RuntimeKind::Rust,
        extension_executable,
        private_file,
        policy,
        ExtensionWorkload::LaunchScale,
    ) {
        Ok(report) => HelperOutcome::ExtensionLaunched {
            observed_mode: report.host_job_mode,
            in_inner_job: report.in_expected_job.passed(),
            extension_exited: report.exit_observed.passed(),
        },
        Err(error) => match error
            .downcast_ref::<JobContextError>()
            .and_then(JobContextError::rejection)
        {
            Some(reason) => HelperOutcome::LaunchRejected { reason },
            None => HelperOutcome::Failed {
                message: format!("{error:#}"),
            },
        },
    };
    serde_json::to_writer(std::io::stdout().lock(), &outcome)
        .context("write nested-Job helper outcome")?;
    Ok(())
}

fn run_context(
    active_context: LaunchJobContext,
    outer_context: OuterJobContext,
    host_executable: &Path,
    extension_executable: &Path,
    private_file: &Path,
    policy: &ContainmentPolicy,
) -> Result<NestedJobContextEvidence> {
    let outer_job = create_outer_job(outer_context)?;
    let mut child = spawn_helper(
        active_context,
        host_executable,
        extension_executable,
        private_file,
    )?;
    // SAFETY: child owns a live process handle and outer_job is a valid Job handle.
    if unsafe { AssignProcessToJobObject(outer_job.raw(), child.as_raw_handle().cast()) } == 0 {
        let assignment_error = std::io::Error::last_os_error();
        drop(child.stdin.take());
        child.wait().context("reap unassigned nested-Job helper")?;
        return Err(assignment_error)
            .with_context(|| format!("assign nested-Job helper to {outer_context:?} outer Job"));
    }
    child
        .stdin
        .take()
        .context("nested-Job helper stdin is unavailable")?
        .write_all(&[START])
        .context("start nested-Job helper")?;

    let output = wait_for_helper(
        child,
        &outer_job,
        policy.workload().nested_job_context_timeout(),
    )?;
    ensure!(
        output.stdout.len() <= policy.pipe().maximum_frame_bytes(),
        "nested-Job helper output exceeded the protocol frame limit"
    );
    let outcome: HelperOutcome =
        serde_json::from_slice(&output.stdout).context("decode nested-Job helper outcome")?;
    let outcome = verify_outcome(outer_context, outcome)?;
    Ok(NestedJobContextEvidence {
        outer_context,
        outcome,
    })
}

fn create_outer_job(context: OuterJobContext) -> Result<OwnedHandle> {
    // SAFETY: null attributes/name create a private Job Object.
    let job = OwnedHandle::new(unsafe { CreateJobObjectW(null(), null()) })?;
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = context.limit_flags();
    super::set_job(job.raw(), JobObjectExtendedLimitInformation, &limits)?;
    if context.has_ui_restrictions() {
        let ui = JOBOBJECT_BASIC_UI_RESTRICTIONS {
            UIRestrictionsClass: JOB_OBJECT_UILIMIT_HANDLES,
        };
        super::set_job(job.raw(), JobObjectBasicUIRestrictions, &ui)?;
    }
    Ok(job)
}

fn spawn_helper(
    active_context: LaunchJobContext,
    host_executable: &Path,
    extension_executable: &Path,
    private_file: &Path,
) -> Result<Child> {
    let mut command = Command::new(host_executable);
    command
        .arg("--nested-job-parent")
        .arg(extension_executable)
        .arg(private_file)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW | active_context.process_creation_flags());
    command.spawn().context("spawn nested-Job helper")
}

fn wait_for_helper(
    mut child: Child,
    outer_job: &OwnedHandle,
    timeout: Duration,
) -> Result<std::process::Output> {
    // SAFETY: child owns a live process handle and timeout comes from validated policy.
    let wait = unsafe {
        WaitForSingleObject(
            child.as_raw_handle().cast(),
            u32::try_from(timeout.as_millis())?,
        )
    };
    if wait == WAIT_TIMEOUT {
        // SAFETY: outer_job is valid and owns the helper process tree.
        if unsafe { TerminateJobObject(outer_job.raw(), HELPER_TIMEOUT_EXIT_CODE) } == 0 {
            return Err(std::io::Error::last_os_error())
                .context("terminate timed-out nested-Job helper");
        }
        child.wait().context("reap timed-out nested-Job helper")?;
        bail!("nested-Job helper exceeded its configured deadline");
    }
    if wait == WAIT_FAILED {
        return Err(std::io::Error::last_os_error()).context("wait for nested-Job helper");
    }
    ensure!(
        wait == WAIT_OBJECT_0,
        "unexpected nested-Job helper wait status {wait:#x}"
    );
    let output = child
        .wait_with_output()
        .context("collect nested-Job helper outcome")?;
    ensure!(
        output.status.success(),
        "nested-Job helper exited with {}",
        output.status
    );
    Ok(output)
}

fn verify_outcome(
    context: OuterJobContext,
    outcome: HelperOutcome,
) -> Result<VerifiedNestedJobOutcome> {
    match (context, outcome) {
        (
            OuterJobContext::NestedWithoutUiRestrictions,
            HelperOutcome::ExtensionLaunched {
                observed_mode: observed_mode @ HostJobMode::Nested,
                in_inner_job,
                extension_exited,
            },
        )
        | (
            OuterJobContext::ExplicitBreakawayWithUiRestrictions,
            HelperOutcome::ExtensionLaunched {
                observed_mode: observed_mode @ HostJobMode::ExplicitBreakaway,
                in_inner_job,
                extension_exited,
            },
        )
        | (
            OuterJobContext::SilentBreakawayWithUiRestrictions,
            HelperOutcome::ExtensionLaunched {
                observed_mode: observed_mode @ HostJobMode::SilentBreakaway,
                in_inner_job,
                extension_exited,
            },
        ) => {
            ensure!(in_inner_job, "extension was not assigned to its inner Job");
            ensure!(extension_exited, "extension did not exit after its session");
            Ok(VerifiedNestedJobOutcome::ExtensionLaunched {
                observed_mode,
                in_inner_job: Verification::Passed,
                extension_exited: Verification::Passed,
            })
        }
        (
            OuterJobContext::UiRestrictedWithoutBreakaway,
            HelperOutcome::LaunchRejected {
                reason: JobContextRejection::UiRestrictionsWithoutBreakaway,
            },
        ) => Ok(VerifiedNestedJobOutcome::LaunchRejected {
            reason: JobContextRejection::UiRestrictionsWithoutBreakaway,
        }),
        (_, HelperOutcome::Failed { message }) => bail!("nested-Job helper failed: {message}"),
        (context, outcome) => bail!("unexpected outcome for {context:?}: {outcome:?}"),
    }
}

fn await_start() -> Result<()> {
    let mut start = [0_u8; 1];
    std::io::stdin()
        .lock()
        .read_exact(&mut start)
        .context("read nested-Job helper start signal")?;
    ensure!(start == [START], "invalid nested-Job helper start signal");
    Ok(())
}
