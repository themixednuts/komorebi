use std::fs::File;
use std::io;
use std::mem::ManuallyDrop;
use std::mem::size_of;
use std::os::windows::io::FromRawHandle;
use std::ptr;

use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::Foundation::ERROR_INSUFFICIENT_BUFFER;
use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::Foundation::HANDLE_FLAG_INHERIT;
use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
use windows_sys::Win32::Foundation::SetHandleInformation;
use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
use windows_sys::Win32::System::JobObjects::CreateJobObjectW;
use windows_sys::Win32::System::JobObjects::JOB_OBJECT_LIMIT_ACTIVE_PROCESS;
use windows_sys::Win32::System::JobObjects::JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
use windows_sys::Win32::System::JobObjects::JOBOBJECT_EXTENDED_LIMIT_INFORMATION;
use windows_sys::Win32::System::JobObjects::JobObjectExtendedLimitInformation;
use windows_sys::Win32::System::JobObjects::SetInformationJobObject;
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::Threading::DeleteProcThreadAttributeList;
use windows_sys::Win32::System::Threading::InitializeProcThreadAttributeList;
use windows_sys::Win32::System::Threading::LPPROC_THREAD_ATTRIBUTE_LIST;
use windows_sys::Win32::System::Threading::TerminateProcess;
use windows_sys::Win32::System::Threading::UpdateProcThreadAttribute;

use super::LpacLaunchError;
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

    pub(super) fn into_file(self) -> File {
        let owned = ManuallyDrop::new(self);
        unsafe {
            // SAFETY: ownership moves from `OwnedHandle` into exactly one `File`.
            File::from_raw_handle(owned.0.cast())
        }
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

pub(super) struct BrokerPipes {
    broker_reader: OwnedHandle,
    broker_writer: OwnedHandle,
    worker_reader: OwnedHandle,
    worker_writer: OwnedHandle,
}

impl BrokerPipes {
    pub(super) fn new() -> Result<Self, LpacLaunchError> {
        let (worker_reader, broker_writer) = inheritable_pipe()?;
        let (broker_reader, worker_writer) = inheritable_pipe()?;
        clear_inheritance(broker_reader.handle())?;
        clear_inheritance(broker_writer.handle())?;
        Ok(Self {
            broker_reader,
            broker_writer,
            worker_reader,
            worker_writer,
        })
    }

    pub(super) const fn worker_handles(&self) -> [HANDLE; 2] {
        [self.worker_reader.handle(), self.worker_writer.handle()]
    }

    pub(super) fn into_files(self) -> (File, File) {
        let Self {
            broker_reader,
            broker_writer,
            worker_reader,
            worker_writer,
        } = self;
        drop(worker_reader);
        drop(worker_writer);
        (broker_reader.into_file(), broker_writer.into_file())
    }
}

fn inheritable_pipe() -> Result<(OwnedHandle, OwnedHandle), LpacLaunchError> {
    let length = u32::try_from(size_of::<SECURITY_ATTRIBUTES>())
        .map_err(|_| LpacLaunchError::StructureSizeOverflow)?;
    let attributes = SECURITY_ATTRIBUTES {
        nLength: length,
        lpSecurityDescriptor: ptr::null_mut(),
        bInheritHandle: 1,
    };
    let mut read = ptr::null_mut();
    let mut write = ptr::null_mut();
    if unsafe {
        // SAFETY: outputs are writable and attributes live through this synchronous call.
        CreatePipe(&raw mut read, &raw mut write, &raw const attributes, 0)
    } == 0
    {
        return Err(LpacLaunchError::windows("CreatePipe"));
    }
    let read = OwnedHandle::new(read)?;
    let write = OwnedHandle::new(write)?;
    Ok((read, write))
}

fn clear_inheritance(handle: HANDLE) -> Result<(), LpacLaunchError> {
    if unsafe {
        // SAFETY: handle is live and the mask changes only its inheritance flag.
        SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0)
    } == 0
    {
        Err(LpacLaunchError::windows("SetHandleInformation"))
    } else {
        Ok(())
    }
}

pub(super) fn terminate(process: HANDLE) {
    unsafe {
        // SAFETY: caller supplies a live process handle; termination is best-effort cleanup.
        TerminateProcess(process, 1);
    }
}
