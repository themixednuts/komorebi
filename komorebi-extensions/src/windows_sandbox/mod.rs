mod path;
mod resources;

use std::io;
use std::mem::size_of;
use std::path::Path;
use std::ptr;

use thiserror::Error;
use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
use windows_sys::Win32::Foundation::WAIT_TIMEOUT;
use windows_sys::Win32::Security::SECURITY_CAPABILITIES;
use windows_sys::Win32::System::JobObjects::AssignProcessToJobObject;
use windows_sys::Win32::System::Threading::CREATE_SUSPENDED;
use windows_sys::Win32::System::Threading::CreateProcessW;
use windows_sys::Win32::System::Threading::EXTENDED_STARTUPINFO_PRESENT;
use windows_sys::Win32::System::Threading::GetExitCodeProcess;
use windows_sys::Win32::System::Threading::PROC_THREAD_ATTRIBUTE_ALL_APPLICATION_PACKAGES_POLICY;
use windows_sys::Win32::System::Threading::PROC_THREAD_ATTRIBUTE_CHILD_PROCESS_POLICY;
use windows_sys::Win32::System::Threading::PROC_THREAD_ATTRIBUTE_MITIGATION_POLICY;
use windows_sys::Win32::System::Threading::PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES;
use windows_sys::Win32::System::Threading::PROCESS_INFORMATION;
use windows_sys::Win32::System::Threading::ResumeThread;
use windows_sys::Win32::System::Threading::STARTUPINFOEXW;
use windows_sys::Win32::System::Threading::WaitForSingleObject;
use windows_sys::Win32::System::WindowsProgramming::PROCESS_CREATION_ALL_APPLICATION_PACKAGES_OPT_OUT;
use windows_sys::Win32::System::WindowsProgramming::PROCESS_CREATION_CHILD_PROCESS_RESTRICTED;

use self::path::WidePath;
use self::path::WideString;
use self::resources::AppContainerSid;
use self::resources::OwnedHandle;
use self::resources::ProcessAttributes;
use self::resources::WorkerJob;
use self::resources::terminate;
use crate::SandboxIdentity;

const ATTRIBUTE_COUNT: u32 = 4;
const PROBE_TIMEOUT_MILLIS: u32 = 15_000;
const RESUME_FAILED: u32 = u32::MAX;
// Public processthreadsapi.h SDK macros are C shift expressions, so windows-rs metadata cannot
// generate them as Rust constants.
const STRICT_HANDLE_CHECKS_ALWAYS_ON: u64 = 1 << 24;
const WIN32K_SYSTEM_CALL_DISABLE_ALWAYS_ON: u64 = 1 << 28;
const EXTENSION_POINT_DISABLE_ALWAYS_ON: u64 = 1 << 32;
const PROHIBIT_DYNAMIC_CODE_ALWAYS_ON: u64 = 1 << 36;
const CREATION_MITIGATIONS: u64 = STRICT_HANDLE_CHECKS_ALWAYS_ON
    | WIN32K_SYSTEM_CALL_DISABLE_ALWAYS_ON
    | EXTENSION_POINT_DISABLE_ALWAYS_ON
    | PROHIBIT_DYNAMIC_CODE_ALWAYS_ON;

/// Launches trusted extension-worker code before any untrusted source is admitted.
pub struct LpacWorkerLauncher {
    identity: SandboxIdentity,
}

impl LpacWorkerLauncher {
    #[must_use]
    pub const fn new(identity: SandboxIdentity) -> Self {
        Self { identity }
    }

    /// Launches the worker and requires it to attest every containment property.
    pub fn launch_probe(&self, worker: &Path) -> Result<VerifiedLpacWorker, LpacLaunchError> {
        let worker = WidePath::new(worker)?;
        let identity = WideString::new(self.identity.as_str());
        let sid = AppContainerSid::open_or_create(&identity)?;
        let job = WorkerJob::new()?;

        let security_capabilities = SECURITY_CAPABILITIES {
            AppContainerSid: sid.as_ptr(),
            Capabilities: ptr::null_mut(),
            CapabilityCount: 0,
            Reserved: 0,
        };
        let all_packages_policy = PROCESS_CREATION_ALL_APPLICATION_PACKAGES_OPT_OUT;
        let child_process_policy = PROCESS_CREATION_CHILD_PROCESS_RESTRICTED;
        let mut attributes = ProcessAttributes::new(ATTRIBUTE_COUNT)?;
        attributes.update(
            PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES,
            &security_capabilities,
        )?;
        attributes.update(
            PROC_THREAD_ATTRIBUTE_ALL_APPLICATION_PACKAGES_POLICY,
            &all_packages_policy,
        )?;
        attributes.update(
            PROC_THREAD_ATTRIBUTE_CHILD_PROCESS_POLICY,
            &child_process_policy,
        )?;
        attributes.update(
            PROC_THREAD_ATTRIBUTE_MITIGATION_POLICY,
            &CREATION_MITIGATIONS,
        )?;

        let mut startup = STARTUPINFOEXW::default();
        startup.StartupInfo.cb = u32::try_from(size_of::<STARTUPINFOEXW>())
            .map_err(|_| LpacLaunchError::StructureSizeOverflow)?;
        startup.lpAttributeList = attributes.as_ptr();
        let mut process_info = PROCESS_INFORMATION::default();
        let created = unsafe {
            // SAFETY: pointers refer to initialized storage alive through the call; the explicit
            // application path is NUL-terminated and no handles are inherited.
            CreateProcessW(
                worker.as_ptr(),
                ptr::null_mut(),
                ptr::null(),
                ptr::null(),
                0,
                EXTENDED_STARTUPINFO_PRESENT | CREATE_SUSPENDED,
                ptr::null(),
                ptr::null(),
                &raw const startup.StartupInfo,
                &raw mut process_info,
            )
        };
        if created == 0 {
            return Err(LpacLaunchError::windows("CreateProcessW"));
        }
        Self::verify_created_worker(process_info, &job)
    }

    fn verify_created_worker(
        process_info: PROCESS_INFORMATION,
        job: &WorkerJob,
    ) -> Result<VerifiedLpacWorker, LpacLaunchError> {
        let process = OwnedHandle::new(process_info.hProcess)?;
        let thread = OwnedHandle::new(process_info.hThread)?;
        if unsafe {
            // SAFETY: both handles are live and owned for this call.
            AssignProcessToJobObject(job.handle(), process.handle())
        } == 0
        {
            let error = LpacLaunchError::windows("AssignProcessToJobObject");
            terminate(process.handle());
            return Err(error);
        }
        if unsafe {
            // SAFETY: this is the suspended primary thread returned by CreateProcessW.
            ResumeThread(thread.handle())
        } == RESUME_FAILED
        {
            let error = LpacLaunchError::windows("ResumeThread");
            terminate(process.handle());
            return Err(error);
        }
        drop(thread);

        let wait = unsafe {
            // SAFETY: the process handle remains live while waiting.
            WaitForSingleObject(process.handle(), PROBE_TIMEOUT_MILLIS)
        };
        if wait == WAIT_TIMEOUT {
            terminate(process.handle());
            return Err(LpacLaunchError::ProbeTimeout);
        }
        if wait != WAIT_OBJECT_0 {
            let error = LpacLaunchError::windows("WaitForSingleObject");
            terminate(process.handle());
            return Err(error);
        }
        let mut exit_code = 0;
        if unsafe {
            // SAFETY: the signalled process is live and output is writable.
            GetExitCodeProcess(process.handle(), &raw mut exit_code)
        } == 0
        {
            return Err(LpacLaunchError::windows("GetExitCodeProcess"));
        }
        if exit_code != 0 {
            return Err(LpacLaunchError::ProbeRejected(exit_code));
        }

        Ok(VerifiedLpacWorker(()))
    }
}

/// Typestate proof returned only after the worker validates its own token and mitigations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VerifiedLpacWorker(());

macro_rules! verified_property {
    ($name:ident) => {
        #[must_use]
        pub const fn $name(self) -> bool {
            true
        }
    };
}

impl VerifiedLpacWorker {
    verified_property!(is_app_container);
    verified_property!(is_less_privileged);
    verified_property!(has_low_integrity);
    verified_property!(has_no_capabilities);
    verified_property!(denies_child_processes);
    verified_property!(disables_win32k);
    verified_property!(prohibits_dynamic_code);
    verified_property!(is_job_contained);
}

#[derive(Debug, Error)]
pub enum LpacLaunchError {
    #[error("extension worker path must be absolute and contain no NUL")]
    InvalidWorkerPath,
    #[error("{operation} failed with HRESULT {hresult:#010x}")]
    Hresult {
        operation: &'static str,
        hresult: i32,
    },
    #[error("{operation} failed: {source}")]
    Windows {
        operation: &'static str,
        #[source]
        source: io::Error,
    },
    #[error("LPAC worker containment probe timed out")]
    ProbeTimeout,
    #[error("LPAC worker rejected containment with probe code {0}")]
    ProbeRejected(u32),
    #[error("Windows returned an invalid process handle")]
    InvalidHandle,
    #[error("a Windows structure size cannot be represented by the target API")]
    StructureSizeOverflow,
}

impl LpacLaunchError {
    fn windows(operation: &'static str) -> Self {
        Self::Windows {
            operation,
            source: io::Error::last_os_error(),
        }
    }
}
