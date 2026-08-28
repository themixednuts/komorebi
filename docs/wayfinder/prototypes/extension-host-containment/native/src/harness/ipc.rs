use std::fs::{self, File};
use std::mem::size_of;
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use std::ptr::null;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};
use windows_sys::Win32::Foundation::{
    ERROR_IO_PENDING, ERROR_NOT_FOUND, ERROR_OPERATION_ABORTED, ERROR_PIPE_CONNECTED, GetLastError,
    HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::Storage::FileSystem::{ReadFile, WriteFile};
use windows_sys::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows_sys::Win32::System::Pipes::ConnectNamedPipe;
use windows_sys::Win32::System::Threading::{
    CreateEventW, GetExitCodeProcess, ResetEvent, WaitForMultipleObjects, WaitForSingleObject,
};

use crate::protocol::{ChildFrame, FrameCodec, HostFrame};
use crate::windows::OwnedHandle;

pub(super) struct PipeChannel {
    pipe: File,
    codec: FrameCodec,
    read_event: OwnedHandle,
    write_event: OwnedHandle,
    operation_timeout: Duration,
    state: ChannelState,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ChannelState {
    AwaitingChild,
    HostMaySend,
    Closed,
}

#[derive(Debug)]
pub(super) enum ReceiveError {
    Deadline,
    Read(std::io::Error),
    State(&'static str),
}

impl std::fmt::Display for ReceiveError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Deadline => formatter.write_str("timed out waiting for child frame"),
            Self::Read(error) => write!(formatter, "read child frame: {error}"),
            Self::State(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for ReceiveError {}

#[derive(Debug)]
pub(super) enum SendError {
    Deadline,
    Encode(std::io::Error),
    Write(std::io::Error),
    State(&'static str),
}

impl std::fmt::Display for SendError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Deadline => formatter.write_str("timed out writing host frame"),
            Self::Encode(error) => write!(formatter, "encode host frame: {error}"),
            Self::Write(error) => write!(formatter, "write host frame: {error}"),
            Self::State(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for SendError {}

impl PipeChannel {
    pub(super) fn new(pipe: File, codec: FrameCodec, operation_timeout: Duration) -> Result<Self> {
        // SAFETY: null security/name and manual-reset initial-unsignaled mode are valid.
        let read_event = OwnedHandle::new(unsafe { CreateEventW(null(), 1, 0, null()) })?;
        // SAFETY: null security/name and manual-reset initial-unsignaled mode are valid.
        let write_event = OwnedHandle::new(unsafe { CreateEventW(null(), 1, 0, null()) })?;
        Ok(Self {
            pipe,
            codec,
            read_event,
            write_event,
            operation_timeout,
            state: ChannelState::AwaitingChild,
        })
    }

    pub(super) fn receive(
        &mut self,
        timeout: Duration,
    ) -> std::result::Result<ChildFrame, ReceiveError> {
        if self.state == ChannelState::Closed {
            return Err(ReceiveError::State("pipe channel is no longer usable"));
        }
        self.state = ChannelState::AwaitingChild;
        let deadline = Deadline::after(timeout).map_err(map_receive_transfer)?;
        let mut length = [0_u8; size_of::<u32>()];
        if let Err(error) = read_exact(
            self.pipe.as_raw_handle().cast(),
            self.read_event.raw(),
            &mut length,
            deadline,
        ) {
            self.state = ChannelState::Closed;
            return Err(map_receive_transfer(error));
        }
        let Ok(length) = usize::try_from(u32::from_le_bytes(length)) else {
            self.state = ChannelState::Closed;
            return Err(ReceiveError::Read(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "frame length does not fit usize",
            )));
        };
        if length > self.codec.limit().bytes() {
            self.state = ChannelState::Closed;
            return Err(ReceiveError::Read(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "frame too large",
            )));
        }
        let mut payload = vec![0_u8; length];
        if let Err(error) = read_exact(
            self.pipe.as_raw_handle().cast(),
            self.read_event.raw(),
            &mut payload,
            deadline,
        ) {
            self.state = ChannelState::Closed;
            return Err(map_receive_transfer(error));
        }
        let frame = match self.codec.decode(&payload) {
            Ok(frame) => frame,
            Err(error) => {
                self.state = ChannelState::Closed;
                return Err(ReceiveError::Read(error));
            }
        };
        self.state = ChannelState::HostMaySend;
        Ok(frame)
    }

    pub(super) fn send(&mut self, frame: &HostFrame) -> std::result::Result<(), SendError> {
        if self.state == ChannelState::Closed {
            return Err(SendError::State("pipe channel is no longer usable"));
        }
        if self.state != ChannelState::HostMaySend {
            return Err(SendError::State(
                "cannot write before receiving a child frame",
            ));
        }
        let encoded = self.codec.encode(frame).map_err(SendError::Encode)?;
        if let Err(error) = write_all(
            self.pipe.as_raw_handle().cast(),
            self.write_event.raw(),
            &encoded,
            Deadline::after(self.operation_timeout).map_err(map_send_transfer)?,
        ) {
            self.state = ChannelState::Closed;
            return Err(map_send_transfer(error));
        }
        Ok(())
    }
}

pub(super) fn connect_or_child_exit(
    pipe: HANDLE,
    process: HANDLE,
    error_file: &Path,
    timeout: Duration,
) -> Result<()> {
    let timeout_ms = u32::try_from(timeout.as_millis())?;
    // SAFETY: null security/name and manual-reset initial-unsignaled mode are valid.
    let connected_event = OwnedHandle::new(unsafe { CreateEventW(null(), 1, 0, null()) })?;
    let mut overlapped = OVERLAPPED {
        hEvent: connected_event.raw(),
        ..Default::default()
    };
    // SAFETY: pipe was created for overlapped I/O and overlapped remains live until settled below.
    if unsafe { ConnectNamedPipe(pipe, &raw mut overlapped) } != 0 {
        return Ok(());
    }
    // SAFETY: GetLastError is called immediately after ConnectNamedPipe.
    let connect_error = unsafe { GetLastError() };
    if connect_error == ERROR_PIPE_CONNECTED {
        return Ok(());
    }
    ensure!(
        connect_error == ERROR_IO_PENDING,
        "connect named pipe: {}",
        std::io::Error::from_raw_os_error(connect_error.cast_signed())
    );
    let wait_handles = [connected_event.raw(), process];
    // SAFETY: both handles remain valid through this bounded wait.
    let wait = unsafe {
        WaitForMultipleObjects(
            u32::try_from(wait_handles.len())?,
            wait_handles.as_ptr(),
            0,
            timeout_ms,
        )
    };
    let wait_error = (wait != WAIT_OBJECT_0 && wait != WAIT_OBJECT_0 + 1 && wait != WAIT_TIMEOUT)
        .then(std::io::Error::last_os_error);
    if wait == WAIT_OBJECT_0 {
        let mut transferred = 0_u32;
        // SAFETY: the connection event is signaled and transferred is writable.
        if unsafe { GetOverlappedResult(pipe, &raw const overlapped, &raw mut transferred, 0) } == 0
        {
            return Err(std::io::Error::last_os_error()).context("complete named-pipe connection");
        }
        return Ok(());
    }
    // SAFETY: pipe and overlapped identify the one pending connection operation.
    if unsafe { CancelIoEx(pipe, &raw const overlapped) } == 0 {
        let error = std::io::Error::last_os_error();
        ensure!(
            error.raw_os_error() == Some(ERROR_NOT_FOUND.cast_signed()),
            "cancel named-pipe connection: {error}"
        );
    }
    let mut transferred = 0_u32;
    // SAFETY: waiting here settles cancellation before overlapped leaves this stack frame.
    if unsafe { GetOverlappedResult(pipe, &raw const overlapped, &raw mut transferred, 1) } == 0 {
        let error = std::io::Error::last_os_error();
        ensure!(
            error.raw_os_error() == Some(ERROR_OPERATION_ABORTED.cast_signed()),
            "settle named-pipe connection cancellation: {error}"
        );
    }
    if wait == WAIT_OBJECT_0 + 1 {
        let mut exit_code = 0_u32;
        // SAFETY: process is a valid process handle and exit_code is writable.
        if unsafe { GetExitCodeProcess(process, &raw mut exit_code) } == 0 {
            return Err(std::io::Error::last_os_error()).context("read LPAC child exit code");
        }
        let detail =
            fs::read_to_string(error_file).unwrap_or_else(|_| "no child error record".to_owned());
        bail!("LPAC child exited before pipe authentication (exit code {exit_code:#x}): {detail}");
    }
    if wait == WAIT_TIMEOUT {
        bail!("timed out waiting for LPAC child pipe connection");
    }
    bail!(
        "WaitForMultipleObjects failed: {}",
        wait_error.context("missing wait failure")?
    )
}

#[derive(Clone, Copy)]
struct Deadline(Instant);

impl Deadline {
    fn after(timeout: Duration) -> std::result::Result<Self, TransferError> {
        Instant::now()
            .checked_add(timeout)
            .map(Self)
            .ok_or_else(|| {
                TransferError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "I/O deadline exceeds Instant range",
                ))
            })
    }

    fn remaining_milliseconds(self) -> std::result::Result<u32, TransferError> {
        let remaining = self
            .0
            .checked_duration_since(Instant::now())
            .ok_or(TransferError::Deadline)?;
        let milliseconds = remaining.as_millis().max(1);
        u32::try_from(milliseconds).map_err(|error| {
            TransferError::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, error))
        })
    }
}

enum TransferError {
    Deadline,
    Io(std::io::Error),
}

fn map_receive_transfer(error: TransferError) -> ReceiveError {
    match error {
        TransferError::Deadline => ReceiveError::Deadline,
        TransferError::Io(error) => ReceiveError::Read(error),
    }
}

fn map_send_transfer(error: TransferError) -> SendError {
    match error {
        TransferError::Deadline => SendError::Deadline,
        TransferError::Io(error) => SendError::Write(error),
    }
}

fn read_exact(
    pipe: HANDLE,
    event: HANDLE,
    mut bytes: &mut [u8],
    deadline: Deadline,
) -> std::result::Result<(), TransferError> {
    while !bytes.is_empty() {
        let transferred = read_once(pipe, event, bytes, deadline)?;
        if transferred == 0 {
            return Err(TransferError::Io(std::io::Error::from(
                std::io::ErrorKind::UnexpectedEof,
            )));
        }
        let (_, remaining) = bytes.split_at_mut(transferred);
        bytes = remaining;
    }
    Ok(())
}

fn write_all(
    pipe: HANDLE,
    event: HANDLE,
    mut bytes: &[u8],
    deadline: Deadline,
) -> std::result::Result<(), TransferError> {
    while !bytes.is_empty() {
        let transferred = write_once(pipe, event, bytes, deadline)?;
        if transferred == 0 {
            return Err(TransferError::Io(std::io::Error::from(
                std::io::ErrorKind::WriteZero,
            )));
        }
        bytes = &bytes[transferred..];
    }
    Ok(())
}

fn read_once(
    pipe: HANDLE,
    event: HANDLE,
    bytes: &mut [u8],
    deadline: Deadline,
) -> std::result::Result<usize, TransferError> {
    let mut overlapped = prepare_overlapped(event)?;
    let mut immediate = 0_u32;
    // SAFETY: pipe was opened for overlapped reads, bytes is writable, and overlapped lives until
    // this operation is completed or cancelled and settled.
    let started = unsafe {
        ReadFile(
            pipe,
            bytes.as_mut_ptr(),
            u32::try_from(bytes.len()).map_err(int_error)?,
            &raw mut immediate,
            &raw mut overlapped,
        )
    };
    finish_transfer(pipe, &mut overlapped, immediate, started != 0, deadline)
}

fn write_once(
    pipe: HANDLE,
    event: HANDLE,
    bytes: &[u8],
    deadline: Deadline,
) -> std::result::Result<usize, TransferError> {
    let mut overlapped = prepare_overlapped(event)?;
    let mut immediate = 0_u32;
    // SAFETY: pipe was opened for overlapped writes, bytes is readable, and overlapped lives until
    // this operation is completed or cancelled and settled.
    let started = unsafe {
        WriteFile(
            pipe,
            bytes.as_ptr(),
            u32::try_from(bytes.len()).map_err(int_error)?,
            &raw mut immediate,
            &raw mut overlapped,
        )
    };
    finish_transfer(pipe, &mut overlapped, immediate, started != 0, deadline)
}

fn prepare_overlapped(event: HANDLE) -> std::result::Result<OVERLAPPED, TransferError> {
    // SAFETY: event is a live manual-reset event owned by the channel.
    if unsafe { ResetEvent(event) } == 0 {
        return Err(TransferError::Io(std::io::Error::last_os_error()));
    }
    Ok(OVERLAPPED {
        hEvent: event,
        ..Default::default()
    })
}

fn finish_transfer(
    pipe: HANDLE,
    overlapped: &mut OVERLAPPED,
    immediate: u32,
    started_synchronously: bool,
    deadline: Deadline,
) -> std::result::Result<usize, TransferError> {
    if started_synchronously {
        return usize::try_from(immediate).map_err(int_error);
    }
    let error = std::io::Error::last_os_error();
    if error.raw_os_error() != Some(ERROR_IO_PENDING.cast_signed()) {
        return Err(TransferError::Io(error));
    }
    let remaining = match deadline.remaining_milliseconds() {
        Ok(remaining) => remaining,
        Err(error) => {
            settle_cancelled_transfer(pipe, overlapped)?;
            return Err(error);
        }
    };
    // SAFETY: overlapped's event remains live and uniquely identifies this pending operation.
    let wait = unsafe { WaitForSingleObject(overlapped.hEvent, remaining) };
    if wait == WAIT_TIMEOUT {
        settle_cancelled_transfer(pipe, overlapped)?;
        return Err(TransferError::Deadline);
    }
    if wait != WAIT_OBJECT_0 {
        let error = std::io::Error::last_os_error();
        settle_cancelled_transfer(pipe, overlapped)?;
        return Err(TransferError::Io(error));
    }
    completed_transfer(pipe, overlapped)
}

fn completed_transfer(
    pipe: HANDLE,
    overlapped: &mut OVERLAPPED,
) -> std::result::Result<usize, TransferError> {
    let mut transferred = 0_u32;
    // SAFETY: the operation's event is signaled and output is writable.
    if unsafe { GetOverlappedResult(pipe, overlapped, &raw mut transferred, 0) } == 0 {
        return Err(TransferError::Io(std::io::Error::last_os_error()));
    }
    usize::try_from(transferred).map_err(int_error)
}

fn settle_cancelled_transfer(
    pipe: HANDLE,
    overlapped: &mut OVERLAPPED,
) -> std::result::Result<(), TransferError> {
    // SAFETY: pipe and overlapped identify the one pending operation owned by this stack frame.
    if unsafe { CancelIoEx(pipe, overlapped) } == 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(ERROR_NOT_FOUND.cast_signed()) {
            return Err(TransferError::Io(error));
        }
    }
    let mut transferred = 0_u32;
    // SAFETY: waiting here guarantees the cancelled/racing operation no longer touches its stack
    // OVERLAPPED or caller-owned buffer before this function returns.
    if unsafe { GetOverlappedResult(pipe, overlapped, &raw mut transferred, 1) } == 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(ERROR_OPERATION_ABORTED.cast_signed()) {
            return Err(TransferError::Io(error));
        }
    }
    Ok(())
}

fn int_error(error: std::num::TryFromIntError) -> TransferError {
    TransferError::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, error))
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::os::windows::io::FromRawHandle;
    use std::ptr::{null, null_mut};
    use std::time::{Duration, Instant};

    use uuid::Uuid;
    use windows_sys::Win32::Foundation::{ERROR_PIPE_CONNECTED, GENERIC_WRITE, GetLastError};
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_FLAG_OVERLAPPED, OPEN_EXISTING, PIPE_ACCESS_INBOUND,
    };
    use windows_sys::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_TYPE_BYTE, PIPE_WAIT,
    };

    use crate::protocol::{FrameCodec, FrameLimit};
    use crate::windows::{OwnedHandle, wide};

    use super::PipeChannel;

    #[test]
    fn stalled_read_is_cancelled_at_deadline() {
        let (server, _client) = connected_test_pipe();
        let codec = FrameCodec::new(FrameLimit::new(1024).expect("valid test frame limit"));
        let mut channel = PipeChannel::new(server, codec, Duration::from_millis(25))
            .expect("create test pipe channel");
        let started = Instant::now();

        let error = channel
            .receive(Duration::from_millis(25))
            .expect_err("stalled pipe must time out");

        assert!(error.to_string().contains("timed out"));
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    fn connected_test_pipe() -> (File, OwnedHandle) {
        let name = format!(r"\\.\pipe\komorebi-wayfinder-test-{}", Uuid::new_v4());
        let name = wide(&name);
        // SAFETY: name is NUL-terminated and null security creates a private test pipe.
        let server = unsafe {
            CreateNamedPipeW(
                name.as_ptr(),
                PIPE_ACCESS_INBOUND | FILE_FLAG_OVERLAPPED,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
                1,
                1024,
                1024,
                0,
                null(),
            )
        };
        let server = OwnedHandle::new(server).expect("create overlapped test pipe");
        // SAFETY: name is NUL-terminated and the server pipe is available.
        let client = unsafe {
            CreateFileW(
                name.as_ptr(),
                GENERIC_WRITE,
                0,
                null(),
                OPEN_EXISTING,
                0,
                null_mut(),
            )
        };
        let client = OwnedHandle::new(client).expect("connect test pipe client");
        // SAFETY: client already connected; ERROR_PIPE_CONNECTED is the expected race result.
        let connected = unsafe { ConnectNamedPipe(server.raw(), null_mut()) };
        if connected == 0 {
            // SAFETY: called immediately after ConnectNamedPipe.
            assert_eq!(unsafe { GetLastError() }, ERROR_PIPE_CONNECTED);
        }
        // SAFETY: ownership of the server handle transfers to File exactly once.
        let server = unsafe { File::from_raw_handle(server.into_raw()) };
        (server, client)
    }
}
