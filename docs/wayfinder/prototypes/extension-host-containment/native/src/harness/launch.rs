use std::ffi::OsString;
use std::fs::{self, File};
use std::mem::size_of;
use std::os::windows::ffi::OsStringExt;
use std::os::windows::io::FromRawHandle;
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};
use uuid::Uuid;
use windows_sys::Win32::Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows_sys::Win32::Security::{SECURITY_ATTRIBUTES, SECURITY_CAPABILITIES};
use windows_sys::Win32::Storage::FileSystem::{FILE_FLAG_OVERLAPPED, PIPE_ACCESS_DUPLEX};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, IsProcessInJob, TerminateJobObject,
};
use windows_sys::Win32::System::Pipes::{
    CreateNamedPipeW, GetNamedPipeClientProcessId, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS,
    PIPE_TYPE_BYTE, PIPE_WAIT,
};
use windows_sys::Win32::System::Threading::{
    CREATE_NO_WINDOW, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT, CreateProcessAsUserW,
    EXTENDED_STARTUPINFO_PRESENT, GetExitCodeProcess, PROCESS_INFORMATION, ResumeThread,
    STARTUPINFOEXW, WaitForSingleObject,
};
use windows_sys::Win32::System::WindowsProgramming::{
    PROCESS_CREATION_ALL_APPLICATION_PACKAGES_OPT_OUT, PROCESS_CREATION_CHILD_PROCESS_RESTRICTED,
};

use crate::protocol::{
    ChildFacts, ChildFrame, ExtensionWorkload, FaultScenario, FrameCodec, FrameLimit, HostFrame,
    RuntimeKind,
};
use crate::windows::{OwnedHandle, current_user_sid, process_token_identity, wide};

use super::environment::{EnvironmentBlock, EnvironmentEntry};
use super::ipc::{PipeChannel, connect_or_child_exit};
use super::job_context::{HostJobMode, LaunchJobContext};
use super::policy::ContainmentPolicy;
use super::windows_boundary::{
    AppContainerProfile, AttributeList, SecurityDescriptor, create_junction,
};
use super::{WIN32K_DISABLE_ALWAYS_ON, create_restricted_job, private_commit};

pub(super) struct AuthenticatedExtension {
    pub(super) channel: PipeChannel,
    job: OwnedHandle,
    pub(super) process: OwnedHandle,
    pub(super) process_id: u32,
    pub(super) pipe_pid: u32,
    pub(super) pipe_acl_sddl: String,
    pub(super) profile_name: String,
    pub(super) foreign_profile_name: String,
    pub(super) foreign_profile_sid: String,
    pub(super) reparse_link_created: bool,
    pub(super) startup_ms: f64,
    pub(super) private_commit_bytes: usize,
    pub(super) in_expected_job: bool,
    pub(super) host_job_mode: HostJobMode,
    pub(super) facts: ChildFacts,
    pub(super) error_file: PathBuf,
    _profile: AppContainerProfile,
    _foreign_profile: AppContainerProfile,
}

impl AuthenticatedExtension {
    pub(super) fn terminate_tree(&self, exit_code: u32) -> Result<()> {
        // SAFETY: job remains valid and owns the extension process tree.
        if unsafe { TerminateJobObject(self.job.raw(), exit_code) } == 0 {
            return Err(std::io::Error::last_os_error()).context("terminate extension job");
        }
        Ok(())
    }

    pub(super) fn wait_for_exit(&self, timeout: Duration) -> Result<Option<u32>> {
        let timeout_ms = u32::try_from(timeout.as_millis())?;
        // SAFETY: process remains valid through the bounded wait.
        let wait = unsafe { WaitForSingleObject(self.process.raw(), timeout_ms) };
        if wait == WAIT_TIMEOUT {
            return Ok(None);
        }
        ensure!(wait == WAIT_OBJECT_0, "wait for fault child failed");
        let mut exit_code = 0_u32;
        // SAFETY: process is signaled and exit_code is writable.
        if unsafe { GetExitCodeProcess(self.process.raw(), &raw mut exit_code) } == 0 {
            return Err(std::io::Error::last_os_error()).context("read fault child exit code");
        }
        Ok(Some(exit_code))
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) enum ExtensionBehavior {
    Normal(ExtensionWorkload),
    Fault(FaultScenario),
}

struct ExtensionFiles {
    profile: AppContainerProfile,
    foreign_profile: AppContainerProfile,
    staged_executable: PathBuf,
    package_file: PathBuf,
    foreign_file: PathBuf,
    reparse_file: PathBuf,
    error_file: PathBuf,
    reparse_link_created: bool,
}

struct HostPipe {
    handle: OwnedHandle,
    name: String,
    acl_sddl: String,
    codec: FrameCodec,
}

struct RestrictedProcess {
    files: ExtensionFiles,
    pipe: HostPipe,
    job: OwnedHandle,
    process: OwnedHandle,
    process_id: u32,
    in_expected_job: bool,
    host_job_mode: HostJobMode,
    nonce: Uuid,
    launch_started: Instant,
}

pub(super) fn launch(
    runtime: RuntimeKind,
    executable: &Path,
    private_file: &Path,
    policy: &ContainmentPolicy,
    behavior: ExtensionBehavior,
    generation: crate::protocol::ExtensionGeneration,
) -> Result<AuthenticatedExtension> {
    let job_context = LaunchJobContext::detect()?;
    let files = ExtensionFiles::stage(runtime, executable, policy)?;
    let pipe = HostPipe::create(&files.profile, policy)?;
    let process =
        RestrictedProcess::spawn(files, pipe, private_file, policy, behavior, job_context)?;
    process.authenticate(runtime, policy, generation)
}

impl ExtensionFiles {
    fn stage(runtime: RuntimeKind, executable: &Path, policy: &ContainmentPolicy) -> Result<Self> {
        let profile = AppContainerProfile::create(runtime, policy)?;
        let foreign_profile = AppContainerProfile::create(runtime, policy)?;
        let foreign_file = foreign_profile
            .folder
            .join("LocalState")
            .join("foreign-private.txt");
        fs::create_dir_all(
            foreign_file
                .parent()
                .context("foreign file has no parent")?,
        )?;
        fs::write(&foreign_file, b"private to a different extension identity")?;
        let staged_bin = profile.folder.join("LocalState").join("bin");
        fs::create_dir_all(&staged_bin)?;
        let staged_executable = staged_bin.join(
            executable
                .file_name()
                .context("extension executable has no file name")?,
        );
        fs::copy(executable, &staged_executable)?;
        let local_state = profile.folder.join("LocalState");
        let package_file = local_state.join(wtf16_probe_file_name());
        let error_file = local_state.join("child-error.txt");
        fs::write(&package_file, b"readable only by this extension identity")?;
        let reparse_directory = local_state.join("foreign-reparse");
        create_junction(
            &reparse_directory,
            foreign_file
                .parent()
                .context("foreign file has no parent")?,
        )?;
        let reparse_link_created = true;
        let reparse_file = reparse_directory.join("foreign-private.txt");
        Ok(Self {
            profile,
            foreign_profile,
            staged_executable,
            package_file,
            foreign_file,
            reparse_file,
            error_file,
            reparse_link_created,
        })
    }
}

fn wtf16_probe_file_name() -> OsString {
    let mut name = OsString::from("package-readable-");
    name.push(OsString::from_wide(&[0xD800]));
    name.push(".txt");
    name
}

impl HostPipe {
    fn create(profile: &AppContainerProfile, policy: &ContainmentPolicy) -> Result<Self> {
        let name = format!(r"\\.\pipe\komorebi-wayfinder-{}", Uuid::new_v4());
        let name_wide = wide(&name)?;
        let (descriptor, acl_sddl) =
            SecurityDescriptor::pipe_for(&current_user_sid()?, &profile.sid_string)?;
        let security = SECURITY_ATTRIBUTES {
            nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>())?,
            lpSecurityDescriptor: descriptor.0,
            bInheritHandle: 0,
        };
        let pipe = policy.pipe();
        // SAFETY: name and descriptor live through the call; sizes and modes are valid.
        let handle = OwnedHandle::new(unsafe {
            CreateNamedPipeW(
                name_wide.as_ptr(),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                pipe.buffer_bytes(),
                pipe.buffer_bytes(),
                u32::try_from(pipe.operation_timeout().as_millis())?,
                &raw const security,
            )
        })?;
        Ok(Self {
            handle,
            name,
            acl_sddl,
            codec: FrameCodec::new(FrameLimit::new(pipe.maximum_frame_bytes())?),
        })
    }
}

impl RestrictedProcess {
    fn spawn(
        files: ExtensionFiles,
        pipe: HostPipe,
        private_file: &Path,
        policy: &ContainmentPolicy,
        behavior: ExtensionBehavior,
        job_context: LaunchJobContext,
    ) -> Result<Self> {
        let job = create_restricted_job(policy.job(), job_context.ui_mode())?;
        let nonce = Uuid::new_v4();
        let launch_started = Instant::now();
        let process_info = create_suspended_process(
            &files,
            &pipe.name,
            &nonce,
            private_file,
            policy,
            behavior,
            job_context.process_creation_flags(),
        )?;
        let process = OwnedHandle::new(process_info.hProcess)?;
        let thread = OwnedHandle::new(process_info.hThread)?;
        // SAFETY: process is suspended and both handles are valid.
        if unsafe { AssignProcessToJobObject(job.raw(), process.raw()) } == 0 {
            return Err(std::io::Error::last_os_error()).context("assign extension to job");
        }
        let mut in_job = 0;
        // SAFETY: handles are valid and in_job is writable.
        if unsafe { IsProcessInJob(process.raw(), job.raw(), &raw mut in_job) } == 0 {
            return Err(std::io::Error::last_os_error()).context("verify job membership");
        }
        // SAFETY: thread is the suspended primary thread from CreateProcessAsUserW.
        let resume_result = unsafe { ResumeThread(thread.raw()) };
        ensure!(resume_result != u32::MAX, "resume extension thread failed");
        Ok(Self {
            files,
            pipe,
            job,
            process,
            process_id: process_info.dwProcessId,
            in_expected_job: in_job != 0,
            host_job_mode: job_context.mode(),
            nonce,
            launch_started,
        })
    }

    fn authenticate(
        self,
        runtime: RuntimeKind,
        policy: &ContainmentPolicy,
        generation: crate::protocol::ExtensionGeneration,
    ) -> Result<AuthenticatedExtension> {
        super::trace("host:wait_connect");
        connect_or_child_exit(
            self.pipe.handle.raw(),
            self.process.raw(),
            &self.files.error_file,
            policy.pipe().connect_timeout(),
        )?;
        super::trace("host:connected");
        let pipe_pid = authenticated_pipe_pid(
            &self.pipe.handle,
            &self.process,
            self.process_id,
            &self.files.profile.sid_string,
        )?;
        // SAFETY: ownership transfers from OwnedHandle to File exactly once.
        let pipe_file = unsafe { File::from_raw_handle(self.pipe.handle.into_raw()) };
        let mut channel = PipeChannel::new(
            pipe_file,
            self.pipe.codec,
            policy.pipe().operation_timeout(),
        )?;
        let facts = authenticate_hello(
            &mut channel,
            runtime,
            self.process_id,
            &self.files.profile.sid_string,
            &self.nonce,
            policy,
            generation,
        )?;
        let private_commit_bytes = private_commit(self.process.raw())?;
        Ok(AuthenticatedExtension {
            channel,
            job: self.job,
            process: self.process,
            profile_name: self.files.profile.name.clone(),
            foreign_profile_name: self.files.foreign_profile.name.clone(),
            foreign_profile_sid: self.files.foreign_profile.sid_string.clone(),
            reparse_link_created: self.files.reparse_link_created,
            startup_ms: self.launch_started.elapsed().as_secs_f64() * 1_000.0,
            private_commit_bytes,
            in_expected_job: self.in_expected_job,
            host_job_mode: self.host_job_mode,
            process_id: self.process_id,
            pipe_pid,
            pipe_acl_sddl: self.pipe.acl_sddl,
            facts,
            error_file: self.files.error_file.clone(),
            _profile: self.files.profile,
            _foreign_profile: self.files.foreign_profile,
        })
    }
}

fn create_suspended_process(
    files: &ExtensionFiles,
    pipe_name: &str,
    nonce: &Uuid,
    private_file: &Path,
    policy: &ContainmentPolicy,
    behavior: ExtensionBehavior,
    additional_creation_flags: u32,
) -> Result<PROCESS_INFORMATION> {
    let capabilities = SECURITY_CAPABILITIES {
        AppContainerSid: files.profile.sid,
        Capabilities: files.profile.capabilities.entries.as_ptr().cast_mut(),
        CapabilityCount: u32::try_from(files.profile.capabilities.entries.len())?,
        Reserved: 0,
    };
    let process = policy.process();
    let all_packages = u32::from(process.opt_out_all_application_packages())
        * PROCESS_CREATION_ALL_APPLICATION_PACKAGES_OPT_OUT;
    let mitigation = [
        u64::from(process.disable_win32k()) * WIN32K_DISABLE_ALWAYS_ON,
        0,
    ];
    let child =
        u32::from(process.restrict_child_processes()) * PROCESS_CREATION_CHILD_PROCESS_RESTRICTED;
    let mut attributes = AttributeList::create(&capabilities, &all_packages, &mitigation, &child)?;
    let mut startup = STARTUPINFOEXW::default();
    startup.StartupInfo.cb = u32::try_from(size_of::<STARTUPINFOEXW>())?;
    startup.lpAttributeList = attributes.raw();
    let environment = child_environment(files, pipe_name, nonce, private_file, policy, behavior)?;
    let application = wide(files.staged_executable.as_os_str())?;
    let current_directory = wide(files.profile.folder.as_os_str())?;
    let mut process_info = PROCESS_INFORMATION::default();
    // SAFETY: pointers reference live NUL-terminated buffers and initialized structures. Supplying
    // the executable separately avoids reparsing a quoted display string as a command line.
    if unsafe {
        CreateProcessAsUserW(
            null_mut(),
            application.as_ptr(),
            null_mut(),
            null(),
            null(),
            0,
            CREATE_SUSPENDED
                | CREATE_NO_WINDOW
                | EXTENDED_STARTUPINFO_PRESENT
                | CREATE_UNICODE_ENVIRONMENT
                | additional_creation_flags,
            environment.as_ptr().cast(),
            current_directory.as_ptr(),
            &raw const startup.StartupInfo,
            &raw mut process_info,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error()).context("create LPAC extension process");
    }
    Ok(process_info)
}

fn child_environment(
    files: &ExtensionFiles,
    pipe_name: &str,
    nonce: &Uuid,
    private_file: &Path,
    policy: &ContainmentPolicy,
    behavior: ExtensionBehavior,
) -> Result<EnvironmentBlock> {
    let parent_pid = std::process::id().to_string();
    let nonce = nonce.to_string();
    let private_verbatim = std::fs::canonicalize(private_file)
        .with_context(|| format!("resolve private probe path {}", private_file.display()))?;
    let frame_limit = policy.pipe().maximum_frame_bytes().to_string();
    let pipe_timeout = policy.pipe().connect_timeout().as_millis().to_string();
    let echo_samples = policy.workload().echo_samples().to_string();
    let trace = std::env::var_os("WAYFINDER_TRACE").unwrap_or_default();
    let (fault_scenario, workload) = match behavior {
        ExtensionBehavior::Normal(workload) => (String::new(), workload),
        ExtensionBehavior::Fault(scenario) => (
            serde_json::to_string(&scenario)?,
            ExtensionWorkload::LaunchScale,
        ),
    };
    let workload = serde_json::to_string(&workload)?;
    let allocation_chunk = policy.faults().allocation_chunk_bytes().to_string();
    EnvironmentBlock::build(
        vec![
            EnvironmentEntry::new("KOMOREBI_PROTOTYPE_PIPE", pipe_name),
            EnvironmentEntry::new("KOMOREBI_PROTOTYPE_NONCE", nonce),
            EnvironmentEntry::new(
                "KOMOREBI_PROTOTYPE_PACKAGE_FILE",
                files.package_file.as_os_str(),
            ),
            EnvironmentEntry::new("KOMOREBI_PROTOTYPE_DENIED_FILE", private_file.as_os_str()),
            EnvironmentEntry::new(
                "KOMOREBI_PROTOTYPE_DENIED_VERBATIM_FILE",
                private_verbatim.as_os_str(),
            ),
            EnvironmentEntry::new(
                "KOMOREBI_PROTOTYPE_FOREIGN_FILE",
                files.foreign_file.as_os_str(),
            ),
            EnvironmentEntry::new(
                "KOMOREBI_PROTOTYPE_REPARSE_FILE",
                files.reparse_file.as_os_str(),
            ),
            EnvironmentEntry::new("KOMOREBI_PROTOTYPE_PARENT_PID", parent_pid),
            EnvironmentEntry::new(
                "KOMOREBI_PROTOTYPE_ERROR_FILE",
                files.error_file.as_os_str(),
            ),
            EnvironmentEntry::new("KOMOREBI_PROTOTYPE_FRAME_LIMIT", frame_limit),
            EnvironmentEntry::new("KOMOREBI_PROTOTYPE_PIPE_TIMEOUT_MS", pipe_timeout),
            EnvironmentEntry::new("KOMOREBI_PROTOTYPE_ECHO_SAMPLES", echo_samples),
            EnvironmentEntry::new("KOMOREBI_PROTOTYPE_TRACE", trace),
            EnvironmentEntry::new("KOMOREBI_PROTOTYPE_FAULT_SCENARIO", fault_scenario),
            EnvironmentEntry::new("KOMOREBI_PROTOTYPE_WORKLOAD", workload),
            EnvironmentEntry::new(
                "KOMOREBI_PROTOTYPE_ALLOCATION_CHUNK_BYTES",
                allocation_chunk,
            ),
        ],
        std::env::var_os("WAYFINDER_FULL_ENV").is_some(),
    )
}

fn authenticated_pipe_pid(
    pipe: &OwnedHandle,
    process: &OwnedHandle,
    process_id: u32,
    package_sid: &str,
) -> Result<u32> {
    let mut pipe_pid = 0_u32;
    // SAFETY: pipe is connected and pipe_pid is writable.
    if unsafe { GetNamedPipeClientProcessId(pipe.raw(), &raw mut pipe_pid) } == 0 {
        return Err(std::io::Error::last_os_error()).context("query pipe client PID");
    }
    ensure!(pipe_pid == process_id, "pipe client PID mismatch");
    let identity = process_token_identity(process)?;
    ensure!(
        identity.app_container && identity.less_privileged_app_container,
        "pipe client process token is not LPAC"
    );
    ensure!(
        identity.package_sid == package_sid,
        "pipe client process token has the wrong AppContainer SID"
    );
    Ok(pipe_pid)
}

fn authenticate_hello(
    channel: &mut PipeChannel,
    runtime: RuntimeKind,
    process_id: u32,
    package_sid: &str,
    nonce: &Uuid,
    policy: &ContainmentPolicy,
    generation: crate::protocol::ExtensionGeneration,
) -> Result<ChildFacts> {
    super::trace("host:wait_hello");
    let first = channel.receive(policy.pipe().operation_timeout())?;
    let ChildFrame::Hello {
        nonce: child_nonce,
        runtime: child_runtime,
        facts,
    } = first
    else {
        bail!("first child frame was not hello");
    };
    ensure!(nonce_matches(&child_nonce, nonce), "pipe nonce mismatch");
    ensure!(facts.pid == process_id, "child-reported PID mismatch");
    ensure!(runtime == child_runtime, "runtime mismatch");
    ensure!(
        facts.app_container && facts.less_privileged_app_container,
        "child is not LPAC"
    );
    ensure!(
        facts.package_sid == package_sid,
        "child AppContainer SID mismatch"
    );
    channel.send(&HostFrame::Welcome { generation })?;
    super::trace("host:welcome_sent");
    Ok(facts)
}

fn nonce_matches(actual: &Uuid, expected: &Uuid) -> bool {
    actual
        .as_bytes()
        .iter()
        .zip(expected.as_bytes())
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
}

#[cfg(test)]
mod tests {
    use std::os::windows::ffi::OsStrExt;

    use uuid::Uuid;

    use super::{nonce_matches, wtf16_probe_file_name};

    #[test]
    fn fixed_width_nonce_comparison_accepts_only_exact_match() {
        let expected = Uuid::from_u128(1);
        assert!(nonce_matches(&expected, &Uuid::from_u128(1)));
        assert!(!nonce_matches(&expected, &Uuid::from_u128(2)));
    }

    #[test]
    fn package_probe_name_contains_an_unpaired_surrogate() {
        assert!(
            wtf16_probe_file_name()
                .encode_wide()
                .any(|unit| unit == 0xD800)
        );
    }
}
