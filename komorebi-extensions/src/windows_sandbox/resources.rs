use std::io;
use std::mem::size_of;
use std::ptr;

use windows_core::HRESULT;
use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::Foundation::ERROR_ALREADY_EXISTS;
use windows_sys::Win32::Foundation::ERROR_INSUFFICIENT_BUFFER;
use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
use windows_sys::Win32::Security::FreeSid;
use windows_sys::Win32::Security::Isolation::CreateAppContainerProfile;
use windows_sys::Win32::Security::Isolation::DeriveAppContainerSidFromAppContainerName;
use windows_sys::Win32::Security::PSID;
use windows_sys::Win32::System::JobObjects::CreateJobObjectW;
use windows_sys::Win32::System::JobObjects::JOB_OBJECT_LIMIT_ACTIVE_PROCESS;
use windows_sys::Win32::System::JobObjects::JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
use windows_sys::Win32::System::JobObjects::JOBOBJECT_EXTENDED_LIMIT_INFORMATION;
use windows_sys::Win32::System::JobObjects::JobObjectExtendedLimitInformation;
use windows_sys::Win32::System::JobObjects::SetInformationJobObject;
use windows_sys::Win32::System::Threading::DeleteProcThreadAttributeList;
use windows_sys::Win32::System::Threading::InitializeProcThreadAttributeList;
use windows_sys::Win32::System::Threading::LPPROC_THREAD_ATTRIBUTE_LIST;
use windows_sys::Win32::System::Threading::TerminateProcess;
use windows_sys::Win32::System::Threading::UpdateProcThreadAttribute;

use super::LpacLaunchError;
use super::path::WideString;

pub(super) struct AppContainerSid(PSID);

impl AppContainerSid {
    pub(super) fn open_or_create(identity: &WideString) -> Result<Self, LpacLaunchError> {
        let display_name = WideString::new("Komorebi extension worker");
        let description = WideString::new("Isolated Komorebi Lua extension worker");
        let mut sid = ptr::null_mut();
        let result = unsafe {
            // SAFETY: strings are NUL-terminated and the output pointer is writable.
            CreateAppContainerProfile(
                identity.as_ptr(),
                display_name.as_ptr(),
                description.as_ptr(),
                ptr::null(),
                0,
                &raw mut sid,
            )
        };
        if result == HRESULT::from_win32(ERROR_ALREADY_EXISTS).0 {
            let derive_result = unsafe {
                // SAFETY: identity is NUL-terminated and the output pointer is writable.
                DeriveAppContainerSidFromAppContainerName(identity.as_ptr(), &raw mut sid)
            };
            if derive_result < 0 {
                return Err(LpacLaunchError::Hresult {
                    operation: "DeriveAppContainerSidFromAppContainerName",
                    hresult: derive_result,
                });
            }
        } else if result < 0 {
            return Err(LpacLaunchError::Hresult {
                operation: "CreateAppContainerProfile",
                hresult: result,
            });
        }
        if sid.is_null() {
            return Err(LpacLaunchError::InvalidHandle);
        }
        Ok(Self(sid))
    }

    pub(super) const fn as_ptr(&self) -> PSID {
        self.0
    }
}

impl Drop for AppContainerSid {
    fn drop(&mut self) {
        unsafe {
            // SAFETY: allocated by an AppContainer SID API and freed once here.
            FreeSid(self.0);
        }
    }
}

pub(super) struct ProcessAttributes {
    storage: Vec<usize>,
}

impl ProcessAttributes {
    pub(super) fn new(count: u32) -> Result<Self, LpacLaunchError> {
        let mut bytes = 0;
        unsafe {
            // SAFETY: a null first call is the documented size query.
            InitializeProcThreadAttributeList(ptr::null_mut(), count, 0, &raw mut bytes);
        }
        if io::Error::last_os_error().raw_os_error()
            != Some(ERROR_INSUFFICIENT_BUFFER.cast_signed())
        {
            return Err(LpacLaunchError::windows(
                "InitializeProcThreadAttributeList(size)",
            ));
        }
        let words = bytes.div_ceil(size_of::<usize>());
        let mut storage = vec![0; words];
        let initialized = unsafe {
            // SAFETY: storage is aligned, writable, and sized by the preceding query.
            InitializeProcThreadAttributeList(storage.as_mut_ptr().cast(), count, 0, &raw mut bytes)
        };
        if initialized == 0 {
            return Err(LpacLaunchError::windows(
                "InitializeProcThreadAttributeList",
            ));
        }
        Ok(Self { storage })
    }

    pub(super) fn update<T>(&mut self, attribute: u32, value: &T) -> Result<(), LpacLaunchError> {
        let updated = unsafe {
            // SAFETY: initialized list and value remain alive through process creation.
            UpdateProcThreadAttribute(
                self.as_ptr(),
                0,
                attribute as usize,
                ptr::from_ref(value).cast(),
                size_of::<T>(),
                ptr::null_mut(),
                ptr::null(),
            )
        };
        if updated == 0 {
            Err(LpacLaunchError::windows("UpdateProcThreadAttribute"))
        } else {
            Ok(())
        }
    }

    pub(super) fn as_ptr(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.storage.as_mut_ptr().cast()
    }
}

impl Drop for ProcessAttributes {
    fn drop(&mut self) {
        unsafe {
            // SAFETY: initialized successfully and deleted exactly once.
            DeleteProcThreadAttributeList(self.storage.as_mut_ptr().cast());
        }
    }
}

pub(super) struct WorkerJob(OwnedHandle);

impl WorkerJob {
    pub(super) fn new() -> Result<Self, LpacLaunchError> {
        let handle = unsafe {
            // SAFETY: null security attributes and name create an unnamed job.
            CreateJobObjectW(ptr::null(), ptr::null())
        };
        let handle = OwnedHandle::new(handle)?;
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags =
            JOB_OBJECT_LIMIT_ACTIVE_PROCESS | JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        limits.BasicLimitInformation.ActiveProcessLimit = 1;
        let size = u32::try_from(size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>())
            .map_err(|_| LpacLaunchError::StructureSizeOverflow)?;
        let configured = unsafe {
            // SAFETY: handle is a live job and limits has the declared structure size.
            SetInformationJobObject(
                handle.handle(),
                JobObjectExtendedLimitInformation,
                ptr::from_ref(&limits).cast(),
                size,
            )
        };
        if configured == 0 {
            return Err(LpacLaunchError::windows("SetInformationJobObject"));
        }
        Ok(Self(handle))
    }

    pub(super) const fn handle(&self) -> HANDLE {
        self.0.handle()
    }
}

pub(super) struct OwnedHandle(HANDLE);

impl OwnedHandle {
    pub(super) fn new(handle: HANDLE) -> Result<Self, LpacLaunchError> {
        if handle.is_null() || handle == INVALID_HANDLE_VALUE {
            Err(LpacLaunchError::InvalidHandle)
        } else {
            Ok(Self(handle))
        }
    }

    pub(super) const fn handle(&self) -> HANDLE {
        self.0
    }
}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        unsafe {
            // SAFETY: this wrapper uniquely owns a live kernel handle.
            CloseHandle(self.0);
        }
    }
}

pub(super) fn terminate(process: HANDLE) {
    unsafe {
        // SAFETY: caller supplies a live process handle; termination is best-effort cleanup.
        TerminateProcess(process, 1);
    }
}
