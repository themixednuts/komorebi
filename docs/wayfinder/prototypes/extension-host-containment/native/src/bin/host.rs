use std::collections::HashMap;
use std::ffi::c_void;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::mem::size_of;
use std::net::{TcpStream, ToSocketAddrs};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};
use mlua::{ChunkMode, Lua, LuaOptions, StdLib};
use serde::Serialize;
use uuid::Uuid;
use wayfinder_extension_containment_prototype::protocol::{
    ChildFacts, ChildFrame, HostFrame, ProbeOutcome, RuntimeKind, read_frame, write_frame,
};
use wayfinder_extension_containment_prototype::windows::{
    OwnedHandle, current_user_sid, process_token_identity, sid_to_string, wide,
};
use windows_sys::Win32::Foundation::{
    ERROR_PIPE_CONNECTED, GetLastError, LocalFree, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::Isolation::{
    CreateAppContainerProfile, DeleteAppContainerProfile, GetAppContainerFolderPath,
};
use windows_sys::Win32::Security::{
    DeriveCapabilitySidsFromName, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, SECURITY_CAPABILITIES,
    SID_AND_ATTRIBUTES,
};
use windows_sys::Win32::Storage::FileSystem::PIPE_ACCESS_DUPLEX;
use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::System::IO::CancelSynchronousIo;
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, IsProcessInJob, JOB_OBJECT_CPU_RATE_CONTROL_ENABLE,
    JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP, JOB_OBJECT_LIMIT_ACTIVE_PROCESS,
    JOB_OBJECT_LIMIT_JOB_MEMORY, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOB_OBJECT_UILIMIT_DESKTOP,
    JOB_OBJECT_UILIMIT_DISPLAYSETTINGS, JOB_OBJECT_UILIMIT_EXITWINDOWS,
    JOB_OBJECT_UILIMIT_GLOBALATOMS, JOB_OBJECT_UILIMIT_HANDLES, JOB_OBJECT_UILIMIT_READCLIPBOARD,
    JOB_OBJECT_UILIMIT_SYSTEMPARAMETERS, JOB_OBJECT_UILIMIT_WRITECLIPBOARD,
    JOBOBJECT_BASIC_UI_RESTRICTIONS, JOBOBJECT_CPU_RATE_CONTROL_INFORMATION,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectBasicUIRestrictions,
    JobObjectCpuRateControlInformation, JobObjectExtendedLimitInformation, SetInformationJobObject,
};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, GetNamedPipeClientProcessId, PIPE_READMODE_BYTE,
    PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
};
use windows_sys::Win32::System::ProcessStatus::{
    K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX,
};
use windows_sys::Win32::System::SystemServices::SE_GROUP_ENABLED;
use windows_sys::Win32::System::Threading::{
    CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT, CreateProcessAsUserW,
    DeleteProcThreadAttributeList, EXTENDED_STARTUPINFO_PRESENT, GetExitCodeProcess,
    InitializeProcThreadAttributeList, PROC_THREAD_ATTRIBUTE_ALL_APPLICATION_PACKAGES_POLICY,
    PROC_THREAD_ATTRIBUTE_CHILD_PROCESS_POLICY, PROC_THREAD_ATTRIBUTE_MITIGATION_POLICY,
    PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES, PROCESS_INFORMATION, ResumeThread, STARTUPINFOEXW,
    UpdateProcThreadAttribute, WaitForMultipleObjects, WaitForSingleObject,
};
use windows_sys::Win32::System::WindowsProgramming::{
    PROCESS_CREATION_ALL_APPLICATION_PACKAGES_OPT_OUT, PROCESS_CREATION_CHILD_PROCESS_RESTRICTED,
};

const WIN32K_DISABLE_ALWAYS_ON: u64 = 0x0000_0000_1000_0000;
const CHILD_TIMEOUT_MS: u32 = 15_000;
const PROTOTYPE_PROFILE_PREFIX: &str = "komorebi.wayfinder.";

static PROFILE_CLEANUP_FAILED: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Serialize)]
struct HarnessReport {
    generated_at_unix_ms: u64,
    platform: String,
    boundary: BoundaryEvidence,
    runs: Vec<RunReport>,
    scale: Vec<ScaleReport>,
    shared_host_control: SharedHostControl,
    cleanup: CleanupEvidence,
}

#[derive(Debug, Serialize)]
struct BoundaryEvidence {
    security_identity: &'static str,
    resource_lifetime: &'static str,
    ipc: &'static str,
    dll_search: &'static str,
    experimental_api_used: bool,
}

#[derive(Debug, Serialize)]
struct RunReport {
    runtime: RuntimeKind,
    profile_name: String,
    expected_pid: u32,
    pipe_reported_pid: u32,
    startup_ms: f64,
    private_commit_bytes: usize,
    in_expected_job: bool,
    facts: ChildFacts,
    probes: Vec<ProbeOutcome>,
    echo_rtt_us: Vec<f64>,
    broker_service_us: Vec<f64>,
    storage_cas_roundtrip: bool,
    brokered_http_status: Option<u16>,
    exit_observed: bool,
}

#[derive(Debug, Serialize)]
struct CleanupEvidence {
    profiles_deleted: bool,
    private_file_deleted: bool,
    pipe_handles_closed: bool,
}

#[derive(Debug, Serialize)]
struct ScaleReport {
    process_count: usize,
    cohort_wall_ms: f64,
    authenticated_ready_p50_ms: f64,
    authenticated_ready_p99_ms: f64,
    aggregate_private_commit_bytes: usize,
    echo_rtt_p99_us: f64,
    forbidden_probes_allowed: usize,
    all_exited: bool,
}

#[derive(Debug, Serialize)]
struct SharedHostControl {
    lua_contexts: usize,
    cohort_startup_ms: f64,
    incremental_private_commit_bytes: usize,
    in_process_noop_p99_us: f64,
    blast_radius_extensions: usize,
    isolation_boundary: &'static str,
}

struct AppContainerProfile {
    name: String,
    name_wide: Vec<u16>,
    sid: *mut c_void,
    sid_string: String,
    folder: PathBuf,
    capabilities: CapabilitySet,
}

struct CapabilitySet {
    entries: Vec<SID_AND_ATTRIBUTES>,
    allocations: Vec<(*mut *mut c_void, u32)>,
}

impl CapabilitySet {
    fn derive(names: &[&str]) -> Result<Self> {
        let mut result = Self {
            entries: Vec::new(),
            allocations: Vec::new(),
        };
        for name in names {
            let name = wide(name);
            let mut group_sids = null_mut();
            let mut group_count = 0_u32;
            let mut capability_sids = null_mut();
            let mut capability_count = 0_u32;
            // SAFETY: name is NUL-terminated and every output pointer/count is writable.
            if unsafe {
                DeriveCapabilitySidsFromName(
                    name.as_ptr(),
                    &raw mut group_sids,
                    &raw mut group_count,
                    &raw mut capability_sids,
                    &raw mut capability_count,
                )
            } == 0
            {
                return Err(std::io::Error::last_os_error()).context("derive LPAC capability SID");
            }
            // SAFETY: the API returned capability_count pointers in capability_sids.
            for index in 0..capability_count as usize {
                // SAFETY: index is bounded by capability_count for the returned pointer array.
                let sid = unsafe { *capability_sids.add(index) };
                result.entries.push(SID_AND_ATTRIBUTES {
                    Sid: sid,
                    Attributes: u32::try_from(SE_GROUP_ENABLED)?,
                });
            }
            result.allocations.push((group_sids, group_count));
            result.allocations.push((capability_sids, capability_count));
        }
        Ok(result)
    }
}

impl Drop for CapabilitySet {
    fn drop(&mut self) {
        for (array, count) in self.allocations.drain(..) {
            if !array.is_null() {
                for index in 0..count as usize {
                    // SAFETY: DeriveCapabilitySidsFromName allocated each SID with LocalAlloc.
                    unsafe { LocalFree(*array.add(index)) };
                }
                // SAFETY: DeriveCapabilitySidsFromName allocated the pointer array with LocalAlloc.
                unsafe { LocalFree(array.cast()) };
            }
        }
    }
}

impl AppContainerProfile {
    fn create(runtime: RuntimeKind) -> Result<Self> {
        let capabilities = CapabilitySet::derive(&["lpacAppExperience"])?;
        let suffix = Uuid::new_v4().simple().to_string();
        let runtime_name = match runtime {
            RuntimeKind::Rust => "rust",
            RuntimeKind::LuaJit => "lua",
        };
        let name = format!("komorebi.wayfinder.{runtime_name}.{suffix}");
        let name_wide = wide(&name);
        let display = wide("Komorebi Wayfinder containment probe");
        let description = wide("Disposable LPAC extension-host prototype");
        let mut sid = null_mut();
        // SAFETY: strings are NUL-terminated, capabilities are intentionally empty, and sid is writable.
        let result = unsafe {
            CreateAppContainerProfile(
                name_wide.as_ptr(),
                display.as_ptr(),
                description.as_ptr(),
                capabilities.entries.as_ptr(),
                u32::try_from(capabilities.entries.len())?,
                &raw mut sid,
            )
        };
        ensure!(
            result >= 0,
            "CreateAppContainerProfile failed: HRESULT {result:#x}"
        );
        // SAFETY: CreateAppContainerProfile returned a valid SID allocation.
        let sid_string = unsafe { sid_to_string(sid)? };
        let sid_wide = wide(&sid_string);
        let mut folder_wide = null_mut();
        // SAFETY: sid_wide is NUL-terminated and folder_wide is writable.
        let result = unsafe { GetAppContainerFolderPath(sid_wide.as_ptr(), &raw mut folder_wide) };
        if result < 0 {
            // SAFETY: CreateAppContainerProfile allocated sid with LocalAlloc.
            unsafe { LocalFree(sid) };
            bail!("GetAppContainerFolderPath failed: HRESULT {result:#x}");
        }
        let folder = PathBuf::from(read_wide(folder_wide));
        // SAFETY: GetAppContainerFolderPath documents CoTaskMemFree for this allocation.
        unsafe { CoTaskMemFree(folder_wide.cast()) };
        Ok(Self {
            name,
            name_wide,
            sid,
            sid_string,
            folder,
            capabilities,
        })
    }
}

impl Drop for AppContainerProfile {
    fn drop(&mut self) {
        // SAFETY: name remains NUL-terminated and sid is owned by this value.
        unsafe {
            LocalFree(self.sid);
            if std::env::var_os("WAYFINDER_RETAIN_PROFILE").is_none() {
                let result = DeleteAppContainerProfile(self.name_wide.as_ptr());
                if result < 0 {
                    PROFILE_CLEANUP_FAILED.store(true, Ordering::Release);
                    eprintln!(
                        "failed to delete profile {}: HRESULT {result:#x}",
                        self.name
                    );
                }
            } else {
                eprintln!(
                    "retained profile {} at {}",
                    self.name,
                    self.folder.display()
                );
            }
        }
    }
}

struct SecurityDescriptor(PSECURITY_DESCRIPTOR);

impl SecurityDescriptor {
    fn pipe_for(user_sid: &str, app_sid: &str) -> Result<Self> {
        let sddl = wide(&format!(
            "D:P(A;;GA;;;SY)(A;;GA;;;{user_sid})(A;;GRGW;;;{app_sid})"
        ));
        let mut descriptor = null_mut();
        // SAFETY: sddl is NUL-terminated and descriptor is writable.
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &raw mut descriptor,
                null_mut(),
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error()).context("build pipe security descriptor");
        }
        Ok(Self(descriptor))
    }
}

impl Drop for SecurityDescriptor {
    fn drop(&mut self) {
        // SAFETY: conversion API allocated this descriptor with LocalAlloc.
        unsafe { LocalFree(self.0) };
    }
}

struct AttributeList {
    storage: Vec<u8>,
}

impl AttributeList {
    fn create(
        capabilities: &SECURITY_CAPABILITIES,
        all_packages_policy: &u32,
        mitigation: &[u64; 2],
        child_policy: &u32,
    ) -> Result<Self> {
        let mut bytes = 0_usize;
        // SAFETY: null is the documented size-query form.
        unsafe { InitializeProcThreadAttributeList(null_mut(), 4, 0, &raw mut bytes) };
        ensure!(bytes > 0, "attribute-list size query failed");
        let mut result = Self {
            storage: vec![0_u8; bytes],
        };
        // SAFETY: storage is writable for the size returned by the query.
        if unsafe { InitializeProcThreadAttributeList(result.raw(), 4, 0, &raw mut bytes) } == 0 {
            return Err(std::io::Error::last_os_error()).context("initialize attribute list");
        }
        result.update(
            PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES,
            std::ptr::from_ref(capabilities).cast::<c_void>(),
            size_of::<SECURITY_CAPABILITIES>(),
        )?;
        result.update(
            PROC_THREAD_ATTRIBUTE_ALL_APPLICATION_PACKAGES_POLICY,
            std::ptr::from_ref(all_packages_policy).cast::<c_void>(),
            size_of::<u32>(),
        )?;
        result.update(
            PROC_THREAD_ATTRIBUTE_MITIGATION_POLICY,
            std::ptr::from_ref(mitigation).cast::<c_void>(),
            size_of::<[u64; 2]>(),
        )?;
        result.update(
            PROC_THREAD_ATTRIBUTE_CHILD_PROCESS_POLICY,
            std::ptr::from_ref(child_policy).cast::<c_void>(),
            size_of::<u32>(),
        )?;
        Ok(result)
    }

    fn raw(&mut self) -> windows_sys::Win32::System::Threading::LPPROC_THREAD_ATTRIBUTE_LIST {
        self.storage.as_mut_ptr().cast()
    }

    fn update(&mut self, attribute: u32, value: *const c_void, size: usize) -> Result<()> {
        // SAFETY: list is initialized and values remain alive through process creation.
        if unsafe {
            UpdateProcThreadAttribute(
                self.raw(),
                0,
                attribute as usize,
                value,
                size,
                null_mut(),
                null(),
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error())
                .with_context(|| format!("set process attribute {attribute}"));
        }
        Ok(())
    }
}

impl Drop for AttributeList {
    fn drop(&mut self) {
        // SAFETY: successful construction initialized this list exactly once.
        unsafe { DeleteProcThreadAttributeList(self.raw()) };
    }
}

fn main() -> Result<()> {
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    if let [flag, profile_name] = arguments.as_slice() {
        ensure!(flag == "--delete-profile", "unknown diagnostic command");
        return delete_prototype_profile(profile_name);
    }
    ensure!(arguments.is_empty(), "unexpected command-line arguments");

    let started = u64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis(),
    )?;
    let executable_dir = std::env::current_exe()?
        .parent()
        .context("host executable has no parent")?
        .to_path_buf();
    let prototype_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .context("manifest has no prototype root")?
        .to_path_buf();
    let results_dir = prototype_root.join("results");
    fs::create_dir_all(&results_dir)?;
    let private_file = results_dir.join(format!("host-private-{}.txt", Uuid::new_v4()));
    fs::write(&private_file, b"must not be readable from LPAC")?;

    let rust = if std::env::var_os("WAYFINDER_DIAGNOSTIC_MINIMAL").is_some() {
        executable_dir.join("containment-minimal-child.exe")
    } else {
        executable_dir.join("containment-rust-child.exe")
    };
    let lua = executable_dir.join("containment-lua-child.exe");
    ensure!(
        rust.is_file() && lua.is_file(),
        "build all release binaries first: cargo build --release --bins"
    );

    let mut runs = Vec::new();
    for (runtime, executable) in [(RuntimeKind::Rust, rust), (RuntimeKind::LuaJit, lua)] {
        runs.push(run_extension(runtime, &executable, &private_file)?);
    }
    let scale = run_scale_cohorts(
        &executable_dir.join("containment-rust-child.exe"),
        &private_file,
    )?;
    let shared_host_control = run_shared_host_control()?;
    fs::remove_file(&private_file)?;
    let report = HarnessReport {
        generated_at_unix_ms: started,
        platform: format!("Windows {}", std::env::consts::ARCH),
        boundary: BoundaryEvidence {
            security_identity: "unique LPAC AppContainer SID with lpacAppExperience only; no registry, COM, clipboard, or network capability",
            resource_lifetime: "no-breakaway Job Object: active-process=1, kill-on-close, 256 MiB, 20% CPU, UI restrictions",
            ipc: "host-owned unqualified named pipe; protected SID DACL; remote clients rejected; PID and nonce authenticated",
            dll_search: "application directory, System32, and explicit user directories only",
            experimental_api_used: false,
        },
        runs,
        scale,
        shared_host_control,
        cleanup: CleanupEvidence {
            profiles_deleted: !PROFILE_CLEANUP_FAILED.load(Ordering::Acquire),
            private_file_deleted: !private_file.exists(),
            pipe_handles_closed: true,
        },
    };
    let output = serde_json::to_vec_pretty(&report)?;
    fs::write(results_dir.join("latest.json"), &output)?;
    println!("{}", String::from_utf8(output)?);
    Ok(())
}

fn delete_prototype_profile(profile_name: &str) -> Result<()> {
    ensure!(
        profile_name.starts_with(PROTOTYPE_PROFILE_PREFIX),
        "refusing to delete a non-prototype AppContainer profile"
    );
    let profile_name = wide(profile_name);
    // SAFETY: profile_name is NUL-terminated and deletion is constrained by the checked prefix.
    let result = unsafe { DeleteAppContainerProfile(profile_name.as_ptr()) };
    ensure!(
        result >= 0,
        "DeleteAppContainerProfile failed: HRESULT {result:#x}"
    );
    Ok(())
}

// This is the executable scenario script: keeping the linear sequence visible makes the measured
// launch, authentication, probes, broker operations, and teardown directly auditable.
#[allow(clippy::too_many_lines)]
fn run_extension(
    runtime: RuntimeKind,
    executable: &Path,
    private_file: &Path,
) -> Result<RunReport> {
    let profile = AppContainerProfile::create(runtime)?;
    let staged_bin = profile.folder.join("LocalState").join("bin");
    fs::create_dir_all(&staged_bin)?;
    let staged_executable = staged_bin.join(
        executable
            .file_name()
            .context("extension executable has no file name")?,
    );
    fs::copy(executable, &staged_executable)?;
    let package_file = profile
        .folder
        .join("LocalState")
        .join("package-readable.txt");
    let error_file = profile.folder.join("LocalState").join("child-error.txt");
    fs::create_dir_all(
        package_file
            .parent()
            .context("package file has no parent")?,
    )?;
    fs::write(&package_file, b"readable only by this extension identity")?;

    let nonce = Uuid::new_v4().to_string();
    let pipe_name = format!(r"\\.\pipe\komorebi-wayfinder-{}", Uuid::new_v4());
    let pipe_name_wide = wide(&pipe_name);
    let descriptor = SecurityDescriptor::pipe_for(&current_user_sid()?, &profile.sid_string)?;
    let security = SECURITY_ATTRIBUTES {
        nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>())?,
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: 0,
    };
    // SAFETY: name and descriptor live through the call; sizes and modes are valid.
    let pipe = OwnedHandle::new(unsafe {
        CreateNamedPipeW(
            pipe_name_wide.as_ptr(),
            PIPE_ACCESS_DUPLEX,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            64 * 1024,
            64 * 1024,
            5_000,
            &raw const security,
        )
    })?;
    let job = create_restricted_job(std::env::var_os("WAYFINDER_NO_JOB_UI").is_none())?;

    let capabilities = SECURITY_CAPABILITIES {
        AppContainerSid: profile.sid,
        Capabilities: profile.capabilities.entries.as_ptr().cast_mut(),
        CapabilityCount: u32::try_from(profile.capabilities.entries.len())?,
        Reserved: 0,
    };
    let all_packages_policy = PROCESS_CREATION_ALL_APPLICATION_PACKAGES_OPT_OUT;
    let mitigation = [
        if std::env::var_os("WAYFINDER_NO_WIN32K").is_some() {
            0
        } else {
            WIN32K_DISABLE_ALWAYS_ON
        },
        0,
    ];
    let child_policy = PROCESS_CREATION_CHILD_PROCESS_RESTRICTED;
    let mut attributes = AttributeList::create(
        &capabilities,
        &all_packages_policy,
        &mitigation,
        &child_policy,
    )?;
    let mut startup = STARTUPINFOEXW::default();
    startup.StartupInfo.cb = u32::try_from(size_of::<STARTUPINFOEXW>())?;
    startup.lpAttributeList = attributes.raw();
    let parent_pid = std::process::id().to_string();
    let package_text = package_file.to_string_lossy();
    let private_text = private_file.to_string_lossy();
    let error_text = error_file.to_string_lossy();
    let environment = environment_block(
        &[
            ("KOMOREBI_PROTOTYPE_PIPE", pipe_name.as_str()),
            ("KOMOREBI_PROTOTYPE_NONCE", nonce.as_str()),
            ("KOMOREBI_PROTOTYPE_PACKAGE_FILE", &package_text),
            ("KOMOREBI_PROTOTYPE_DENIED_FILE", &private_text),
            ("KOMOREBI_PROTOTYPE_PARENT_PID", &parent_pid),
            ("KOMOREBI_PROTOTYPE_ERROR_FILE", &error_text),
        ],
        std::env::var_os("WAYFINDER_FULL_ENV").is_some(),
    );
    let inherit_environment = std::env::var_os("WAYFINDER_INHERIT_ENV").is_some();
    let environment_pointer = if inherit_environment {
        null()
    } else {
        environment.as_ptr().cast()
    };
    let mut command = wide(&format!("\"{}\"", staged_executable.display()));
    let current_directory: Vec<u16> = profile
        .folder
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let mut process = PROCESS_INFORMATION::default();
    let launch_started = Instant::now();
    // SAFETY: pointers reference live NUL-terminated buffers/structures; command is mutable as required.
    if unsafe {
        CreateProcessAsUserW(
            null_mut(),
            null(),
            command.as_mut_ptr(),
            null(),
            null(),
            0,
            CREATE_SUSPENDED
                | EXTENDED_STARTUPINFO_PRESENT
                | if inherit_environment {
                    0
                } else {
                    CREATE_UNICODE_ENVIRONMENT
                },
            environment_pointer,
            current_directory.as_ptr(),
            &raw const startup.StartupInfo,
            &raw mut process,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error()).context("create LPAC extension process");
    }
    let process_handle = OwnedHandle::new(process.hProcess)?;
    let thread_handle = OwnedHandle::new(process.hThread)?;
    // SAFETY: process is suspended and both handles are valid.
    if unsafe { AssignProcessToJobObject(job.raw(), process_handle.raw()) } == 0 {
        return Err(std::io::Error::last_os_error()).context("assign extension to job");
    }
    let mut in_job = 0;
    // SAFETY: handles are valid and in_job is writable.
    if unsafe { IsProcessInJob(process_handle.raw(), job.raw(), &raw mut in_job) } == 0 {
        return Err(std::io::Error::last_os_error()).context("verify job membership");
    }
    // SAFETY: thread is the suspended primary thread from CreateProcessW.
    let resume_result = unsafe { ResumeThread(thread_handle.raw()) };
    ensure!(resume_result != u32::MAX, "resume extension thread failed");

    connect_or_child_exit(pipe.raw(), process_handle.raw(), &error_file)?;
    let mut pipe_pid = 0_u32;
    // SAFETY: pipe is connected and pipe_pid is writable.
    if unsafe { GetNamedPipeClientProcessId(pipe.raw(), &raw mut pipe_pid) } == 0 {
        return Err(std::io::Error::last_os_error()).context("query pipe client PID");
    }
    ensure!(pipe_pid == process.dwProcessId, "pipe client PID mismatch");
    let peer_identity = process_token_identity(&process_handle)?;
    ensure!(
        peer_identity.app_container && peer_identity.less_privileged_app_container,
        "pipe client process token is not LPAC"
    );
    ensure!(
        peer_identity.package_sid == profile.sid_string,
        "pipe client process token has the wrong AppContainer SID"
    );
    // SAFETY: ownership transfers from OwnedHandle to File exactly once.
    let mut pipe_file = unsafe { File::from_raw_handle(pipe.into_raw()) };
    let first: ChildFrame = read_frame(&mut pipe_file)?;
    let ChildFrame::Hello {
        nonce: child_nonce,
        runtime: child_runtime,
        facts,
    } = first
    else {
        bail!("first child frame was not hello");
    };
    ensure!(child_nonce == nonce, "pipe nonce mismatch");
    ensure!(
        facts.pid == process.dwProcessId,
        "child-reported PID mismatch"
    );
    ensure!(
        matches!(
            (runtime, child_runtime),
            (RuntimeKind::Rust, RuntimeKind::Rust) | (RuntimeKind::LuaJit, RuntimeKind::LuaJit)
        ),
        "runtime mismatch"
    );
    ensure!(
        facts.app_container && facts.less_privileged_app_container,
        "child is not LPAC"
    );
    ensure!(
        facts.package_sid == profile.sid_string,
        "child AppContainer SID mismatch"
    );
    write_frame(&mut pipe_file, &HostFrame::Welcome { generation: 1 })?;
    let startup_ms = launch_started.elapsed().as_secs_f64() * 1_000.0;
    let private_commit_bytes = private_commit(process_handle.raw())?;

    let mut broker_service_us = Vec::new();
    let mut storage: HashMap<String, (u64, Vec<u8>)> = HashMap::new();
    let mut storage_cas_roundtrip = false;
    let mut brokered_http_status = None;
    let mut probes = Vec::new();
    let echo_rtt_us = loop {
        let frame: ChildFrame = read_frame(&mut pipe_file)?;
        let service_started = Instant::now();
        match frame {
            ChildFrame::Echo {
                sequence,
                sent_ticks,
            } => {
                write_frame(
                    &mut pipe_file,
                    &HostFrame::Echoed {
                        sequence,
                        sent_ticks,
                    },
                )?;
            }
            ChildFrame::StoragePut {
                request,
                key,
                expected_revision,
                value,
            } => {
                let current = storage.get(&key).map_or(0, |(revision, _)| *revision);
                if current == expected_revision && value.len() <= 256 * 1024 {
                    let revision = current + 1;
                    storage.insert(key, (revision, value));
                    storage_cas_roundtrip = true;
                    write_frame(
                        &mut pipe_file,
                        &HostFrame::StorageStored { request, revision },
                    )?;
                } else {
                    write_frame(
                        &mut pipe_file,
                        &HostFrame::Rejected {
                            request: Some(request),
                            code: "storage_conflict_or_limit".to_owned(),
                        },
                    )?;
                }
            }
            ChildFrame::StorageGet { request, key } => {
                if let Some((revision, value)) = storage.get(&key) {
                    write_frame(
                        &mut pipe_file,
                        &HostFrame::StorageValue {
                            request,
                            revision: *revision,
                            value: value.clone(),
                        },
                    )?;
                } else {
                    write_frame(
                        &mut pipe_file,
                        &HostFrame::Rejected {
                            request: Some(request),
                            code: "storage_missing".to_owned(),
                        },
                    )?;
                }
            }
            ChildFrame::HttpGet { request, url } => match broker_http(&url) {
                Ok((status, bytes)) => {
                    brokered_http_status = Some(status);
                    write_frame(
                        &mut pipe_file,
                        &HostFrame::HttpResult {
                            request,
                            status,
                            bytes,
                        },
                    )?;
                }
                Err(error) => {
                    write_frame(
                        &mut pipe_file,
                        &HostFrame::Rejected {
                            request: Some(request),
                            code: format!("http_policy_or_transport:{error}"),
                        },
                    )?;
                }
            },
            ChildFrame::ProbeReport { probes: reported } => probes = reported,
            ChildFrame::Goodbye {
                echo_rtt_us: reported,
            } => {
                break reported;
            }
            ChildFrame::Hello { .. } => bail!("duplicate hello"),
        }
        broker_service_us.push(service_started.elapsed().as_secs_f64() * 1_000_000.0);
    };
    drop(pipe_file);
    // SAFETY: process handle is valid and timeout is bounded.
    let exit_observed =
        unsafe { WaitForSingleObject(process_handle.raw(), CHILD_TIMEOUT_MS) } == WAIT_OBJECT_0;
    Ok(RunReport {
        runtime,
        profile_name: profile.name.clone(),
        expected_pid: process.dwProcessId,
        pipe_reported_pid: pipe_pid,
        startup_ms,
        private_commit_bytes,
        in_expected_job: in_job != 0,
        facts,
        probes,
        echo_rtt_us,
        broker_service_us,
        storage_cas_roundtrip,
        brokered_http_status,
        exit_observed,
    })
}

fn create_restricted_job(apply_ui_restrictions: bool) -> Result<OwnedHandle> {
    // SAFETY: null attributes/name create a private job object.
    let job = OwnedHandle::new(unsafe { CreateJobObjectW(null(), null()) })?;
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_ACTIVE_PROCESS
        | JOB_OBJECT_LIMIT_JOB_MEMORY
        | JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    limits.BasicLimitInformation.ActiveProcessLimit = 1;
    limits.JobMemoryLimit = 256 * 1024 * 1024;
    set_job(job.raw(), JobObjectExtendedLimitInformation, &limits)?;
    let mut cpu = JOBOBJECT_CPU_RATE_CONTROL_INFORMATION {
        ControlFlags: JOB_OBJECT_CPU_RATE_CONTROL_ENABLE | JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP,
        ..Default::default()
    };
    // SAFETY: CpuRate is the active union field for hard-cap mode.
    cpu.Anonymous.CpuRate = 2_000;
    set_job(job.raw(), JobObjectCpuRateControlInformation, &cpu)?;
    if apply_ui_restrictions {
        let ui = JOBOBJECT_BASIC_UI_RESTRICTIONS {
            UIRestrictionsClass: JOB_OBJECT_UILIMIT_DESKTOP
                | JOB_OBJECT_UILIMIT_DISPLAYSETTINGS
                | JOB_OBJECT_UILIMIT_EXITWINDOWS
                | JOB_OBJECT_UILIMIT_GLOBALATOMS
                | JOB_OBJECT_UILIMIT_HANDLES
                | JOB_OBJECT_UILIMIT_READCLIPBOARD
                | JOB_OBJECT_UILIMIT_SYSTEMPARAMETERS
                | JOB_OBJECT_UILIMIT_WRITECLIPBOARD,
        };
        set_job(job.raw(), JobObjectBasicUIRestrictions, &ui)?;
    }
    Ok(job)
}

fn connect_or_child_exit(
    pipe: windows_sys::Win32::Foundation::HANDLE,
    process: windows_sys::Win32::Foundation::HANDLE,
    error_file: &Path,
) -> Result<()> {
    let pipe_value = pipe as usize;
    let connector = std::thread::spawn(move || {
        let pipe = pipe_value as windows_sys::Win32::Foundation::HANDLE;
        // SAFETY: the host owns pipe for the thread's lifetime and performs no other I/O until join.
        if unsafe { ConnectNamedPipe(pipe, null_mut()) } != 0 {
            return Ok(());
        }
        // SAFETY: GetLastError is called immediately after ConnectNamedPipe.
        if unsafe { GetLastError() } == ERROR_PIPE_CONNECTED {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    });
    let thread = connector.as_raw_handle() as windows_sys::Win32::Foundation::HANDLE;
    let wait_handles = [thread, process];
    // SAFETY: both handles remain valid through this bounded wait.
    let wait = unsafe {
        WaitForMultipleObjects(
            u32::try_from(wait_handles.len())?,
            wait_handles.as_ptr(),
            0,
            CHILD_TIMEOUT_MS,
        )
    };
    if wait == WAIT_OBJECT_0 {
        return connector
            .join()
            .map_err(|_| anyhow::anyhow!("pipe connector panicked"))?
            .context("connect named pipe");
    }
    // SAFETY: thread is a valid thread handle owned by connector; cancellation only targets its blocking call.
    unsafe { CancelSynchronousIo(thread) };
    let _ = connector.join();
    if wait == WAIT_OBJECT_0 + 1 {
        let mut exit_code = 0_u32;
        // SAFETY: process is a valid process handle and exit_code is writable.
        unsafe { GetExitCodeProcess(process, &raw mut exit_code) };
        let detail =
            fs::read_to_string(error_file).unwrap_or_else(|_| "no child error record".to_owned());
        bail!("LPAC child exited before pipe authentication (exit code {exit_code:#x}): {detail}");
    }
    if wait == WAIT_TIMEOUT {
        bail!("timed out waiting for LPAC child pipe connection");
    }
    bail!(
        "WaitForMultipleObjects failed: {}",
        std::io::Error::last_os_error()
    )
}

fn set_job<T>(job: windows_sys::Win32::Foundation::HANDLE, class: i32, value: &T) -> Result<()> {
    // SAFETY: value matches class and is readable for its advertised size.
    if unsafe {
        SetInformationJobObject(
            job,
            class,
            std::ptr::from_ref(value).cast(),
            u32::try_from(size_of::<T>())?,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error()).context("set Job Object policy");
    }
    Ok(())
}

fn environment_block(extra: &[(&str, &str)], include_all: bool) -> Vec<u16> {
    let mut entries: Vec<(String, String)> = if include_all {
        std::env::vars().collect()
    } else {
        [
            "SYSTEMROOT",
            "WINDIR",
            "COMSPEC",
            "PATH",
            "PATHEXT",
            "USERNAME",
            "USERDOMAIN",
            "USERPROFILE",
            "HOMEDRIVE",
            "HOMEPATH",
            "LOCALAPPDATA",
            "APPDATA",
            "PROGRAMDATA",
            "PROGRAMFILES",
            "PROGRAMFILES(X86)",
            "COMMONPROGRAMFILES",
            "NUMBER_OF_PROCESSORS",
            "OS",
            "PROCESSOR_ARCHITECTURE",
            "TEMP",
            "TMP",
        ]
        .into_iter()
        .filter_map(|key| std::env::var(key).ok().map(|value| (key.to_owned(), value)))
        .collect()
    };
    entries.extend(
        extra
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned())),
    );
    entries.sort_unstable_by(|left, right| {
        left.0
            .to_ascii_uppercase()
            .cmp(&right.0.to_ascii_uppercase())
    });
    let mut block = Vec::new();
    for (key, value) in entries {
        block.extend(format!("{key}={value}").encode_utf16());
        block.push(0);
    }
    block.push(0);
    block
}

fn private_commit(process: windows_sys::Win32::Foundation::HANDLE) -> Result<usize> {
    let mut memory = PROCESS_MEMORY_COUNTERS_EX {
        cb: u32::try_from(size_of::<PROCESS_MEMORY_COUNTERS_EX>())?,
        ..Default::default()
    };
    // SAFETY: memory is writable for cb bytes and process is queryable.
    if unsafe { K32GetProcessMemoryInfo(process, (&raw mut memory).cast(), memory.cb) } == 0 {
        return Err(std::io::Error::last_os_error()).context("query child private commit");
    }
    Ok(memory.PrivateUsage)
}

fn broker_http(url: &str) -> Result<(u16, usize)> {
    ensure!(
        url == "http://example.com/",
        "URL is outside the prototype allowlist"
    );
    let address = ("example.com", 80)
        .to_socket_addrs()?
        .next()
        .context("host DNS returned no example.com address")?;
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(3))?;
    stream.set_read_timeout(Some(Duration::from_secs(3)))?;
    stream.write_all(
        b"GET / HTTP/1.1\r\nHost: example.com\r\nConnection: close\r\nUser-Agent: komorebi-wayfinder/0\r\n\r\n",
    )?;
    let mut response = Vec::new();
    stream.take(1024 * 1024).read_to_end(&mut response)?;
    let first_line = response
        .split(|byte| *byte == b'\n')
        .next()
        .context("HTTP response has no status line")?;
    let status = std::str::from_utf8(first_line)?
        .split_whitespace()
        .nth(1)
        .context("HTTP response has no status")?
        .parse()?;
    Ok((status, response.len()))
}

fn run_scale_cohorts(executable: &Path, private_file: &Path) -> Result<Vec<ScaleReport>> {
    let mut reports = Vec::new();
    for process_count in [1_usize, 4, 16] {
        let cohort_started = Instant::now();
        let mut workers = Vec::with_capacity(process_count);
        for _ in 0..process_count {
            let executable = executable.to_path_buf();
            let private_file = private_file.to_path_buf();
            workers.push(std::thread::spawn(move || {
                run_extension(RuntimeKind::Rust, &executable, &private_file)
            }));
        }
        let mut runs = Vec::with_capacity(process_count);
        for worker in workers {
            runs.push(
                worker
                    .join()
                    .map_err(|_| anyhow::anyhow!("scale worker panicked"))??,
            );
        }
        let mut ready: Vec<_> = runs.iter().map(|run| run.startup_ms).collect();
        let mut rtt: Vec<_> = runs
            .iter()
            .flat_map(|run| run.echo_rtt_us.iter().copied())
            .collect();
        ready.sort_by(f64::total_cmp);
        rtt.sort_by(f64::total_cmp);
        reports.push(ScaleReport {
            process_count,
            cohort_wall_ms: cohort_started.elapsed().as_secs_f64() * 1_000.0,
            authenticated_ready_p50_ms: percentile(&ready, 50, 100),
            authenticated_ready_p99_ms: percentile(&ready, 99, 100),
            aggregate_private_commit_bytes: runs.iter().map(|run| run.private_commit_bytes).sum(),
            echo_rtt_p99_us: percentile(&rtt, 99, 100),
            forbidden_probes_allowed: runs
                .iter()
                .flat_map(|run| &run.probes)
                .filter(|probe| {
                    matches!(probe.expected, wayfinder_extension_containment_prototype::protocol::ExpectedOutcome::Denied)
                        && matches!(probe.observed, wayfinder_extension_containment_prototype::protocol::ObservedOutcome::Allowed)
                })
                .count(),
            all_exited: runs.iter().all(|run| run.exit_observed),
        });
    }
    Ok(reports)
}

fn run_shared_host_control() -> Result<SharedHostControl> {
    // SAFETY: GetCurrentProcess returns a valid pseudo-handle for the current process.
    let process = unsafe { windows_sys::Win32::System::Threading::GetCurrentProcess() };
    let before = private_commit(process)?;
    let started = Instant::now();
    let mut contexts = Vec::with_capacity(16);
    for _ in 0..16 {
        let lua = Lua::new_with(StdLib::NONE, LuaOptions::default())
            .map_err(|error| anyhow::anyhow!("create shared-host LuaJIT control: {error}"))?;
        lua.set_memory_limit(64 * 1024 * 1024)
            .map_err(|error| anyhow::anyhow!("limit shared-host LuaJIT control: {error}"))?;
        let marker: u64 = lua
            .load("return 1")
            .set_mode(ChunkMode::Text)
            .eval()
            .map_err(|error| anyhow::anyhow!("activate shared-host LuaJIT control: {error}"))?;
        ensure!(marker == 1, "shared-host LuaJIT activation mismatch");
        contexts.push(lua);
    }
    let cohort_startup_ms = started.elapsed().as_secs_f64() * 1_000.0;
    let after = private_commit(process)?;
    let mut noops = Vec::new();
    for lua in &contexts {
        let function: mlua::Function = lua
            .load("return function(value) return value end")
            .set_mode(ChunkMode::Text)
            .eval()
            .map_err(|error| anyhow::anyhow!("create shared-host no-op: {error}"))?;
        for value in 0..32_u64 {
            let call_started = Instant::now();
            let returned: u64 = function
                .call(value)
                .map_err(|error| anyhow::anyhow!("call shared-host no-op: {error}"))?;
            std::hint::black_box(returned);
            noops.push(call_started.elapsed().as_secs_f64() * 1_000_000.0);
        }
    }
    noops.sort_by(f64::total_cmp);
    Ok(SharedHostControl {
        lua_contexts: contexts.len(),
        cohort_startup_ms,
        incremental_private_commit_bytes: after.saturating_sub(before),
        in_process_noop_p99_us: percentile(&noops, 99, 100),
        blast_radius_extensions: contexts.len(),
        isolation_boundary: "none: one native crash or memory corruption terminates every context",
    })
}

fn percentile(sorted: &[f64], numerator: usize, denominator: usize) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    debug_assert!(denominator > 0 && numerator <= denominator);
    let scaled = (sorted.len() - 1).saturating_mul(numerator);
    let index = scaled.saturating_add(denominator - 1) / denominator;
    sorted[index.min(sorted.len() - 1)]
}

fn read_wide(pointer: *const u16) -> String {
    let mut length = 0;
    // SAFETY: caller only passes a Windows-allocated NUL-terminated string.
    unsafe {
        while *pointer.add(length) != 0 {
            length += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(pointer, length))
    }
}
