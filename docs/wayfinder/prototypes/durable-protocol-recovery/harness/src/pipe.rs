use std::ffi::OsStr;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::ptr::{null, null_mut};
use std::time::Duration;

use thiserror::Error;
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_IO_PENDING, ERROR_PIPE_CONNECTED, GENERIC_READ, GENERIC_WRITE, GetLastError,
    HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, OPEN_EXISTING,
    PIPE_ACCESS_DUPLEX, ReadFile, WriteFile,
};
use windows_sys::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, GetNamedPipeClientProcessId, PIPE_READMODE_BYTE,
    PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
};
use windows_sys::Win32::System::Threading::{CreateEventW, SetEvent, WaitForSingleObject};

use crate::frame::{FrameError, FrameHeader, HEADER_BYTES, MAX_PAYLOAD_BYTES, PREFACE};

const IO_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug)]
pub struct Pipe(OwnedHandle);

impl Pipe {
    pub fn create_server(name: &OsStr) -> Result<Self, PipeError> {
        let name = wide(name);
        // SAFETY: the name is NUL-terminated; remaining pointers are null by contract.
        let handle = unsafe {
            CreateNamedPipeW(
                name.as_ptr(),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                65_536,
                65_536,
                0,
                null(),
            )
        };
        Ok(Self(OwnedHandle::new(handle)?))
    }

    pub fn connect_client(name: &OsStr) -> Result<Self, PipeError> {
        let name = wide(name);
        // SAFETY: the name is NUL-terminated and the call does not retain it.
        let handle = unsafe {
            CreateFileW(
                name.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                0,
                null(),
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED,
                null_mut(),
            )
        };
        Ok(Self(OwnedHandle::new(handle)?))
    }

    pub fn accept(&self) -> Result<(), PipeError> {
        let event = OwnedHandle::event()?;
        let mut operation = overlapped(event.raw());
        // SAFETY: handle and OVERLAPPED remain alive until the operation settles below.
        if unsafe { ConnectNamedPipe(self.0.raw(), &raw mut operation) } != 0 {
            return Ok(());
        }
        // SAFETY: GetLastError reads thread-local error state immediately after the failed call.
        let error = unsafe { GetLastError() };
        if error == ERROR_PIPE_CONNECTED {
            // SAFETY: event is a valid manual-reset event used only by this operation.
            if unsafe { SetEvent(event.raw()) } == 0 {
                return Err(PipeError::Io(io::Error::last_os_error()));
            }
        } else if error != ERROR_IO_PENDING {
            return Err(PipeError::Io(io::Error::from_raw_os_error(
                error.cast_signed(),
            )));
        }
        settle(self.0.raw(), &mut operation, IO_TIMEOUT).map(|_| ())
    }

    pub fn peer_pid(&self) -> Result<u32, PipeError> {
        let mut pid = 0_u32;
        // SAFETY: this is a connected server pipe and pid is writable.
        if unsafe { GetNamedPipeClientProcessId(self.0.raw(), &raw mut pid) } == 0 {
            return Err(PipeError::Io(io::Error::last_os_error()));
        }
        Ok(pid)
    }

    pub fn write_all(&self, mut bytes: &[u8]) -> Result<(), PipeError> {
        let maximum_chunk = usize::try_from(u32::MAX).map_err(|_| PipeError::Range)?;
        while !bytes.is_empty() {
            let chunk_len = bytes.len().min(maximum_chunk);
            let event = OwnedHandle::event()?;
            let mut operation = overlapped(event.raw());
            // SAFETY: buffer and OVERLAPPED remain alive until settle completes.
            let started = unsafe {
                WriteFile(
                    self.0.raw(),
                    bytes.as_ptr(),
                    u32::try_from(chunk_len).map_err(|_| PipeError::Range)?,
                    null_mut(),
                    &raw mut operation,
                )
            };
            pending_or_complete(started)?;
            let transferred = usize::try_from(settle(self.0.raw(), &mut operation, IO_TIMEOUT)?)
                .map_err(|_| PipeError::Range)?;
            if transferred == 0 || transferred > bytes.len() {
                return Err(PipeError::ZeroWrite);
            }
            bytes = bytes.get(transferred..).ok_or(PipeError::Range)?;
        }
        Ok(())
    }

    pub fn read_exact(&self, mut bytes: &mut [u8]) -> Result<(), PipeError> {
        let maximum_chunk = usize::try_from(u32::MAX).map_err(|_| PipeError::Range)?;
        while !bytes.is_empty() {
            let chunk_len = bytes.len().min(maximum_chunk);
            let event = OwnedHandle::event()?;
            let mut operation = overlapped(event.raw());
            // SAFETY: buffer and OVERLAPPED remain alive until settle completes.
            let started = unsafe {
                ReadFile(
                    self.0.raw(),
                    bytes.as_mut_ptr(),
                    u32::try_from(chunk_len).map_err(|_| PipeError::Range)?,
                    null_mut(),
                    &raw mut operation,
                )
            };
            pending_or_complete(started)?;
            let transferred = usize::try_from(settle(self.0.raw(), &mut operation, IO_TIMEOUT)?)
                .map_err(|_| PipeError::Range)?;
            if transferred == 0 || transferred > bytes.len() {
                return Err(PipeError::UnexpectedEof);
            }
            let (_, rest) = bytes.split_at_mut(transferred);
            bytes = rest;
        }
        Ok(())
    }

    pub fn send_frame(&self, header: FrameHeader, payload: &[u8]) -> Result<(), PipeError> {
        if usize::try_from(header.payload_len).map_err(|_| PipeError::Range)? != payload.len() {
            return Err(PipeError::LengthMismatch);
        }
        self.write_all(&header.encode())?;
        self.write_all(payload)
    }

    pub fn receive_frame(&self) -> Result<(FrameHeader, Vec<u8>), PipeError> {
        let mut header = [0_u8; HEADER_BYTES];
        self.read_exact(&mut header)?;
        let header = FrameHeader::decode(&header)?;
        let length = usize::try_from(header.payload_len).map_err(|_| PipeError::Range)?;
        if length > MAX_PAYLOAD_BYTES {
            return Err(PipeError::LengthMismatch);
        }
        let mut payload = vec![0_u8; length];
        self.read_exact(&mut payload)?;
        Ok((header, payload))
    }

    pub fn handshake_client(&self) -> Result<(), PipeError> {
        self.write_all(&PREFACE)?;
        let mut response = [0_u8; PREFACE.len()];
        self.read_exact(&mut response)?;
        if response != PREFACE {
            return Err(PipeError::Preface);
        }
        Ok(())
    }

    pub fn handshake_server(&self) -> Result<(), PipeError> {
        let mut request = [0_u8; PREFACE.len()];
        self.read_exact(&mut request)?;
        if request != PREFACE {
            return Err(PipeError::Preface);
        }
        self.write_all(&PREFACE)
    }
}

fn pending_or_complete(started: i32) -> Result<(), PipeError> {
    if started != 0 {
        return Ok(());
    }
    // SAFETY: GetLastError reads thread-local error state immediately after I/O initiation.
    let error = unsafe { GetLastError() };
    if error == ERROR_IO_PENDING {
        Ok(())
    } else {
        Err(PipeError::Io(io::Error::from_raw_os_error(
            error.cast_signed(),
        )))
    }
}

fn settle(handle: HANDLE, operation: &mut OVERLAPPED, timeout: Duration) -> Result<u32, PipeError> {
    let timeout = u32::try_from(timeout.as_millis()).map_err(|_| PipeError::Range)?;
    // SAFETY: operation contains a valid event and remains alive for the wait.
    if unsafe { WaitForSingleObject(operation.hEvent, timeout) } != WAIT_OBJECT_0 {
        // SAFETY: this cancels exactly this operation; settlement below still owns its lifetime.
        // GetOverlappedResult is authoritative for completion, including cancellation; the
        // immediate CancelIoEx result cannot safely release any borrowed buffer or OVERLAPPED.
        unsafe { CancelIoEx(handle, operation) };
    }
    let mut transferred = 0_u32;
    // SAFETY: the operation has signaled or was cancelled and all pointers remain live.
    if unsafe { GetOverlappedResult(handle, operation, &raw mut transferred, 1) } == 0 {
        return Err(PipeError::Io(io::Error::last_os_error()));
    }
    Ok(transferred)
}

fn overlapped(event: HANDLE) -> OVERLAPPED {
    // SAFETY: all-zero is the documented initial state for OVERLAPPED; hEvent is then assigned.
    let mut operation: OVERLAPPED = unsafe { std::mem::zeroed() };
    operation.hEvent = event;
    operation
}

fn wide(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(Some(0)).collect()
}

#[derive(Debug)]
struct OwnedHandle(HANDLE);

impl OwnedHandle {
    fn new(handle: HANDLE) -> Result<Self, PipeError> {
        if handle.is_null() || handle == INVALID_HANDLE_VALUE {
            Err(PipeError::Io(io::Error::last_os_error()))
        } else {
            Ok(Self(handle))
        }
    }

    fn event() -> Result<Self, PipeError> {
        // SAFETY: creating an unnamed manual-reset event with no security descriptor is valid.
        Self::new(unsafe { CreateEventW(null(), 1, 0, null()) })
    }

    const fn raw(&self) -> HANDLE {
        self.0
    }
}

// SAFETY: HANDLE ownership is transferred between benchmark threads, not shared concurrently.
unsafe impl Send for OwnedHandle {}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        // SAFETY: this type owns one valid handle and closes it exactly once.
        // CloseHandle has no recoverable action during Drop; ownership still ends here.
        unsafe { CloseHandle(self.0) };
    }
}

#[derive(Debug, Error)]
pub enum PipeError {
    #[error("Windows named-pipe I/O")]
    Io(#[source] io::Error),
    #[error("numeric value is outside a Win32 or Rust range")]
    Range,
    #[error("overlapped write completed without progress")]
    ZeroWrite,
    #[error("overlapped read reached EOF")]
    UnexpectedEof,
    #[error("frame header and payload lengths differ")]
    LengthMismatch,
    #[error("peer sent an invalid protocol preface")]
    Preface,
    #[error(transparent)]
    Frame(#[from] FrameError),
}
