mod path;
mod policy;
mod profile;
mod resources;
mod session;

use std::io;
use std::mem::size_of;
use std::path::Path;
use std::ptr;

use thiserror::Error;
use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
use windows_sys::Win32::Foundation::WAIT_TIMEOUT;
use windows_sys::Win32::System::JobObjects::AssignProcessToJobObject;
use windows_sys::Win32::System::Threading::CREATE_SUSPENDED;
use windows_sys::Win32::System::Threading::CREATE_UNICODE_ENVIRONMENT;
use windows_sys::Win32::System::Threading::CreateProcessW;
use windows_sys::Win32::System::Threading::EXTENDED_STARTUPINFO_PRESENT;
use windows_sys::Win32::System::Threading::GetExitCodeProcess;
use windows_sys::Win32::System::Threading::PROCESS_INFORMATION;
use windows_sys::Win32::System::Threading::ResumeThread;
use windows_sys::Win32::System::Threading::STARTUPINFOEXW;
use windows_sys::Win32::System::Threading::WaitForSingleObject;

use self::path::WidePath;
use self::path::WideString;
use self::policy::with_process_policy;
use self::profile::AppContainerEnvironment;
use self::profile::AppContainerSid;
use self::resources::BrokerPipes;
use self::resources::OwnedHandle;
use self::resources::WorkerJob;
use self::resources::terminate;
pub use self::session::LpacSessionError;
use self::session::NativeWorkerSession;
use crate::PluginId;
use crate::SandboxIdentity;

const PROBE_TIMEOUT_MILLIS: u32 = 15_000;
const RESUME_FAILED: u32 = u32::MAX;

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

        let process_info = with_process_policy(&sid, None, |attributes| {
            let mut startup = STARTUPINFOEXW::default();
            startup.StartupInfo.cb = u32::try_from(size_of::<STARTUPINFOEXW>())
                .map_err(|_| LpacLaunchError::StructureSizeOverflow)?;
            startup.lpAttributeList = attributes.as_ptr();
            let mut process_info = PROCESS_INFORMATION::default();
            let created = unsafe {
                // SAFETY: pointers refer to initialized storage alive through the call; the
                // explicit application path is NUL-terminated and no handles are inherited.
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
                Err(LpacLaunchError::windows("CreateProcessW"))
            } else {
                Ok(process_info)
            }
        })?;
        Self::verify_created_worker(process_info, &job)
    }

    pub(crate) fn launch_session(
        &self,
        worker: &Path,
        plugin: PluginId,
    ) -> Result<NativeWorkerSession, LpacLaunchError> {
        let worker = WidePath::new(worker)?;
        let identity = WideString::new(self.identity.as_str());
        let sid = AppContainerSid::open_or_create(&identity)?;
        let job = WorkerJob::new()?;
        let pipes = BrokerPipes::new()?;
        let worker_handles = pipes.worker_handles();

        let read_handle = worker_handles[0] as usize;
        let write_handle = worker_handles[1] as usize;
        let mut command_line = WideString::new(&format!(
            "komorebi-extension-worker --broker {read_handle} {write_handle}"
        ));
        let environment = AppContainerEnvironment::new(&sid)?;
        let process_info = with_process_policy(&sid, Some(&worker_handles), |attributes| {
            let mut startup = STARTUPINFOEXW::default();
            startup.StartupInfo.cb = u32::try_from(size_of::<STARTUPINFOEXW>())
                .map_err(|_| LpacLaunchError::StructureSizeOverflow)?;
            startup.lpAttributeList = attributes.as_ptr();
            let mut process_info = PROCESS_INFORMATION::default();
            let created = unsafe {
                // SAFETY: all pointers remain live through the call. Only the two handles in the
                // explicit handle list are inheritable by the child.
                CreateProcessW(
                    worker.as_ptr(),
                    command_line.as_mut_ptr(),
                    ptr::null(),
                    ptr::null(),
                    1,
                    EXTENDED_STARTUPINFO_PRESENT | CREATE_SUSPENDED | CREATE_UNICODE_ENVIRONMENT,
                    environment.as_ptr().cast(),
                    ptr::null(),
                    &raw const startup.StartupInfo,
                    &raw mut process_info,
                )
            };
            if created == 0 {
                Err(LpacLaunchError::windows("CreateProcessW"))
            } else {
                Ok(process_info)
            }
        })?;
        let process = Self::activate_created_worker(process_info, &job)?;
        let (reader, writer) = pipes.into_files();
        Ok(NativeWorkerSession::new(
            process, job, reader, writer, plugin,
        ))
    }

    fn verify_created_worker(
        process_info: PROCESS_INFORMATION,
        job: &WorkerJob,
    ) -> Result<VerifiedLpacWorker, LpacLaunchError> {
        let process = Self::activate_created_worker(process_info, job)?;

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

    fn activate_created_worker(
        process_info: PROCESS_INFORMATION,
        job: &WorkerJob,
    ) -> Result<OwnedHandle, LpacLaunchError> {
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
        Ok(process)
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
