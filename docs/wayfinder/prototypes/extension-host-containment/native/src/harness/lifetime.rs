use std::io::{stdin, stdout};
use std::os::windows::io::{AsRawHandle, RawHandle};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Instant;

use anyhow::{Context, Result, bail, ensure};
use uuid::Uuid;
use windows_sys::Win32::Foundation::{HANDLE, WAIT_OBJECT_0};
use windows_sys::Win32::Storage::FileSystem::SYNCHRONIZE;
use windows_sys::Win32::System::Threading::{
    CREATE_NO_WINDOW, GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    WaitForSingleObject,
};

use crate::protocol::{
    FaultScenario, FrameCodec, FrameLimit, HostFrame, ParentControlFrame, ParentExitMode,
    RuntimeKind,
};
use crate::windows::OwnedHandle;

use super::launch::{ExtensionBehavior, launch};
use super::policy::ContainmentPolicy;
use super::report::{ParentLifetimeEvidence, Verification};
use super::windows_boundary::delete_profile;

const PRIVATE_FILE_ENV: &str = "KOMOREBI_LIFETIME_PRIVATE_FILE";

pub(super) fn run_suite(
    executable_dir: &Path,
    private_file: &Path,
    policy: &ContainmentPolicy,
) -> Result<Vec<ParentLifetimeEvidence>> {
    [ParentExitMode::Graceful, ParentExitMode::Abort]
        .into_iter()
        .map(|mode| observe_parent_exit(executable_dir, private_file, policy, mode))
        .collect()
}

pub(super) fn run_parent(mode: ParentExitMode, policy: &ContainmentPolicy) -> Result<()> {
    let private_file = std::env::var_os(PRIVATE_FILE_ENV)
        .map(PathBuf::from)
        .context("missing lifetime private-file path")?;
    let executable_dir = std::env::current_exe()?
        .parent()
        .context("lifetime parent executable has no directory")?
        .to_path_buf();
    let generation = policy.workload().generation();
    let mut extension = launch(
        RuntimeKind::Rust,
        &executable_dir.join("containment-fault-child.exe"),
        &private_file,
        policy,
        ExtensionBehavior::Fault(FaultScenario::IndefiniteWait),
        generation,
    )?;
    extension
        .channel
        .send(&HostFrame::RunFault { generation })?;
    let armed = extension
        .channel
        .receive(policy.pipe().operation_timeout())?;
    ensure!(
        matches!(armed, crate::protocol::ChildFrame::FaultArmed {
            generation: armed_generation,
            scenario: FaultScenario::IndefiniteWait,
        } if armed_generation == generation),
        "lifetime child did not arm its independent kernel wait"
    );
    let nonce = required_nonce_argument()?;
    let codec = FrameCodec::new(FrameLimit::new(policy.pipe().maximum_frame_bytes())?);
    let mut output = stdout().lock();
    codec.write(
        &mut output,
        &ParentControlFrame::Ready {
            nonce,
            mode,
            child_pid: extension.process_id,
            profile_names: [
                extension.profile_name.clone(),
                extension.foreign_profile_name.clone(),
            ],
        },
    )?;
    let acknowledgment: ParentControlFrame = codec.read(&mut stdin().lock())?;
    ensure!(
        matches!(acknowledgment, ParentControlFrame::Acknowledge { nonce: seen } if seen == nonce),
        "lifetime observer acknowledgment mismatch"
    );
    match mode {
        ParentExitMode::Graceful => {
            drop(extension);
            Ok(())
        }
        ParentExitMode::Abort => std::process::abort(),
    }
}

fn observe_parent_exit(
    executable_dir: &Path,
    private_file: &Path,
    policy: &ContainmentPolicy,
    mode: ParentExitMode,
) -> Result<ParentLifetimeEvidence> {
    let nonce = Uuid::new_v4();
    let mut parent = spawn_parent(executable_dir, private_file, mode, nonce)?;
    let codec = FrameCodec::new(FrameLimit::new(policy.pipe().maximum_frame_bytes())?);
    let mut parent_output = parent
        .stdout
        .take()
        .context("lifetime parent stdout was not piped")?;
    let ready: ParentControlFrame = codec.read(&mut parent_output)?;
    let ParentControlFrame::Ready {
        nonce: seen,
        mode: seen_mode,
        child_pid,
        profile_names,
    } = ready
    else {
        bail!("lifetime parent sent acknowledgment before readiness");
    };
    ensure!(
        seen == nonce && seen_mode == mode,
        "lifetime readiness mismatch"
    );
    let child_process = open_observed_child(child_pid)?;
    let mut parent_input = parent
        .stdin
        .take()
        .context("lifetime parent stdin was not piped")?;
    codec.write(
        &mut parent_input,
        &ParentControlFrame::Acknowledge { nonce },
    )?;
    drop(parent_input);

    let parent_exit_code = wait_child_process(&mut parent, policy.pipe().operation_timeout())?;
    let parent_exit_observed = Instant::now();
    let child_exit_code = wait_handle(
        child_process.raw(),
        policy.pipe().operation_timeout(),
        "LPAC child after parent exit",
    )?;
    let child_exit_after_parent_ms = parent_exit_observed.elapsed().as_secs_f64() * 1_000.0;
    for profile_name in &profile_names {
        delete_profile(profile_name, policy)?;
    }
    verify_parent_exit(mode, parent_exit_code)?;
    Ok(ParentLifetimeEvidence {
        mode,
        child_workload: "fault-armed independent infinite kernel wait",
        parent_exit_code,
        child_exit_code,
        child_exit_after_parent_ms,
        process_tree_terminated: Verification::Passed,
        profiles_deleted: Verification::Passed,
    })
}

fn spawn_parent(
    executable_dir: &Path,
    private_file: &Path,
    mode: ParentExitMode,
    nonce: Uuid,
) -> Result<Child> {
    let mut command = Command::new(executable_dir.join("containment-host.exe"));
    command
        .args(["--lifetime-parent", mode.as_str(), &nonce.to_string()])
        .env(PRIVATE_FILE_ENV, private_file.as_os_str())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW);
    command.spawn().context("spawn lifetime parent")
}

fn required_nonce_argument() -> Result<Uuid> {
    std::env::args_os()
        .nth(3)
        .context("missing lifetime nonce")?
        .to_str()
        .context("lifetime nonce is not valid Unicode")?
        .parse()
        .context("invalid lifetime nonce")
}

fn open_observed_child(pid: u32) -> Result<OwnedHandle> {
    // SAFETY: OpenProcess validates the observed PID; requested rights only permit waiting and
    // reading its exit code.
    OwnedHandle::new(unsafe {
        OpenProcess(SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION, 0, pid)
    })
    .context("open LPAC child before parent exit")
}

fn wait_child_process(child: &mut Child, timeout: std::time::Duration) -> Result<u32> {
    let handle = child.as_raw_handle();
    let exit_code = wait_handle(raw_handle(handle), timeout, "lifetime parent")?;
    child.wait().context("reap lifetime parent")?;
    Ok(exit_code)
}

fn raw_handle(handle: RawHandle) -> HANDLE {
    handle.cast()
}

fn wait_handle(handle: HANDLE, timeout: std::time::Duration, label: &str) -> Result<u32> {
    // SAFETY: handle is a live process handle and timeout is bounded by validated policy.
    let wait = unsafe { WaitForSingleObject(handle, u32::try_from(timeout.as_millis())?) };
    ensure!(wait == WAIT_OBJECT_0, "timed out waiting for {label}");
    let mut exit_code = 0_u32;
    // SAFETY: the signaled process handle remains valid and exit_code is writable.
    if unsafe { GetExitCodeProcess(handle, &raw mut exit_code) } == 0 {
        return Err(std::io::Error::last_os_error()).with_context(|| format!("read {label} exit"));
    }
    Ok(exit_code)
}

fn verify_parent_exit(mode: ParentExitMode, exit_code: u32) -> Result<()> {
    match mode {
        ParentExitMode::Graceful => {
            ensure!(exit_code == 0, "graceful parent exit was {exit_code:#x}");
        }
        ParentExitMode::Abort => {
            ensure!(
                exit_code == 0xC000_0409,
                "aborted parent exit was {exit_code:#x}"
            );
        }
    }
    Ok(())
}
