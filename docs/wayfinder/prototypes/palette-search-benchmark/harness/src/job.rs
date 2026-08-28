use std::ffi::c_void;
use std::mem::{size_of, zeroed};

use thiserror::Error;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject,
};

use crate::native::{NativeError, OwnedHandle};

#[derive(Debug)]
pub struct KillOnCloseJob(OwnedHandle);

impl KillOnCloseJob {
    pub fn create() -> Result<Self, JobError> {
        // SAFETY: no security descriptor and no globally visible name are requested.
        let handle = unsafe { CreateJobObjectW(None, None) }?;
        let handle = OwnedHandle::new(handle)?;
        // SAFETY: zero initialization is valid for this C aggregate before selected fields are set.
        let mut limits = unsafe { zeroed::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() };
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: limits has the exact representation and byte length required by this info class.
        unsafe {
            SetInformationJobObject(
                handle.raw(),
                JobObjectExtendedLimitInformation,
                (&raw const limits).cast::<c_void>(),
                u32::try_from(size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>())
                    .map_err(|_| JobError::SizeOverflow)?,
            )
        }?;
        Ok(Self(handle))
    }

    pub fn assign(&self, process: HANDLE) -> Result<(), JobError> {
        // SAFETY: both handles are live and this job owns the child lifetime after assignment.
        unsafe { AssignProcessToJobObject(self.0.raw(), process) }?;
        Ok(())
    }

    pub fn terminate(&self, exit_code: u32) -> Result<(), JobError> {
        // SAFETY: the handle names this owned job; the exit code is measurement-local.
        unsafe { TerminateJobObject(self.0.raw(), exit_code) }?;
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum JobError {
    #[error("Windows Job Object operation failed")]
    Windows(#[from] windows::core::Error),
    #[error("native Job Object handle is invalid")]
    Native(#[from] NativeError),
    #[error("Job Object structure size does not fit the Windows API")]
    SizeOverflow,
}
