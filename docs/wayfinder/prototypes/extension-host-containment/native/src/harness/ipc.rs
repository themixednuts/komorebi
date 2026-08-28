use std::fs::File;
use std::os::windows::io::AsRawHandle;
use std::ptr::null;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use anyhow::{Context, Result, ensure};
use windows_sys::Win32::Foundation::{ERROR_NOT_FOUND, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows_sys::Win32::System::IO::CancelSynchronousIo;
use windows_sys::Win32::System::Threading::{
    CreateEventW, INFINITE, SetEvent, WaitForSingleObject,
};

use crate::protocol::{ChildFrame, FrameCodec, HostFrame};
use crate::windows::OwnedHandle;

pub(super) struct PipeChannel {
    pipe: File,
    codec: FrameCodec,
    frame_ready: OwnedHandle,
    continue_reading: OwnedHandle,
    frame: Arc<Mutex<Option<std::io::Result<ChildFrame>>>>,
    stop: Arc<AtomicBool>,
    reader: Option<JoinHandle<()>>,
    reader_paused: bool,
}

#[derive(Debug)]
pub(super) enum ReceiveError {
    Deadline,
    Wait(std::io::Error),
    Read(std::io::Error),
    State(&'static str),
    Shutdown(std::io::Error),
}

impl std::fmt::Display for ReceiveError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Deadline => formatter.write_str("timed out waiting for child frame"),
            Self::Wait(error) => write!(formatter, "wait for child frame failed: {error}"),
            Self::Read(error) => write!(formatter, "read child frame: {error}"),
            Self::State(message) => formatter.write_str(message),
            Self::Shutdown(error) => write!(formatter, "stop pipe reader: {error}"),
        }
    }
}

impl std::error::Error for ReceiveError {}

impl PipeChannel {
    pub(super) fn new(pipe: File, codec: FrameCodec) -> Result<Self> {
        // SAFETY: null security/name and auto-reset initial-unsignaled mode are valid.
        let frame_ready = OwnedHandle::new(unsafe { CreateEventW(null(), 0, 0, null()) })?;
        // SAFETY: null security/name and auto-reset initial-unsignaled mode are valid.
        let continue_reading = OwnedHandle::new(unsafe { CreateEventW(null(), 0, 0, null()) })?;
        let frame = Arc::new(Mutex::new(None));
        let stop = Arc::new(AtomicBool::new(false));
        let mut reader_pipe = pipe.try_clone().context("duplicate pipe reader handle")?;
        let frame_for_reader = Arc::clone(&frame);
        let stop_for_reader = Arc::clone(&stop);
        let frame_ready_value = frame_ready.raw() as usize;
        let continue_value = continue_reading.raw() as usize;
        let reader = std::thread::spawn(move || {
            let frame_ready = frame_ready_value as windows_sys::Win32::Foundation::HANDLE;
            let continue_reading = continue_value as windows_sys::Win32::Foundation::HANDLE;
            loop {
                let next = codec.read(&mut reader_pipe);
                if stop_for_reader.load(Ordering::Acquire) {
                    break;
                }
                let Ok(mut slot) = frame_for_reader.lock() else {
                    break;
                };
                *slot = Some(next);
                drop(slot);
                // SAFETY: the channel owns frame_ready until this thread joins.
                if unsafe { SetEvent(frame_ready) } == 0 {
                    break;
                }
                // SAFETY: the channel owns continue_reading until this thread joins.
                if unsafe { WaitForSingleObject(continue_reading, INFINITE) } != WAIT_OBJECT_0
                    || stop_for_reader.load(Ordering::Acquire)
                {
                    break;
                }
            }
        });
        Ok(Self {
            pipe,
            codec,
            frame_ready,
            continue_reading,
            frame,
            stop,
            reader: Some(reader),
            reader_paused: false,
        })
    }

    pub(super) fn receive(
        &mut self,
        timeout: Duration,
    ) -> std::result::Result<ChildFrame, ReceiveError> {
        if self.reader_paused {
            // SAFETY: the reader waits on this channel-owned event after publishing each frame.
            let signaled = unsafe { SetEvent(self.continue_reading.raw()) };
            if signaled == 0 {
                return Err(ReceiveError::Wait(std::io::Error::last_os_error()));
            }
            self.reader_paused = false;
        }
        let timeout_ms = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX);
        // SAFETY: frame_ready is valid and timeout_ms is bounded by policy.
        let wait = unsafe { WaitForSingleObject(self.frame_ready.raw(), timeout_ms) };
        if wait == WAIT_TIMEOUT {
            self.stop_reader().map_err(ReceiveError::Shutdown)?;
            return Err(ReceiveError::Deadline);
        }
        if wait != WAIT_OBJECT_0 {
            return Err(ReceiveError::Wait(std::io::Error::last_os_error()));
        }
        self.reader_paused = true;
        self.frame
            .lock()
            .map_err(|_| ReceiveError::State("child frame slot poisoned"))?
            .take()
            .ok_or(ReceiveError::State("reader signaled without a child frame"))?
            .map_err(ReceiveError::Read)
    }

    pub(super) fn send(&mut self, frame: &HostFrame) -> Result<()> {
        ensure!(
            self.reader_paused,
            "cannot write while a pipe read is active"
        );
        self.codec
            .write(&mut self.pipe, frame)
            .context("write host frame")
    }

    fn stop_reader(&mut self) -> std::io::Result<()> {
        let Some(reader) = self.reader.take() else {
            return Ok(());
        };
        self.stop.store(true, Ordering::Release);
        let thread = reader.as_raw_handle() as windows_sys::Win32::Foundation::HANDLE;
        // SAFETY: these handles belong to the live reader and channel. Signaling covers the paused
        // state; cancellation covers the blocking-read state.
        let mut shutdown_error = if unsafe { SetEvent(self.continue_reading.raw()) } == 0 {
            Some(std::io::Error::last_os_error())
        } else {
            None
        };
        // SAFETY: thread is the live reader thread and cancellation is restricted to its current
        // synchronous pipe operation.
        if unsafe { CancelSynchronousIo(thread) } == 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() != Some(i32::try_from(ERROR_NOT_FOUND).unwrap_or(i32::MAX))
                && shutdown_error.is_none()
            {
                shutdown_error = Some(error);
            }
        }
        if reader.join().is_err() && shutdown_error.is_none() {
            shutdown_error = Some(std::io::Error::other("pipe reader panicked"));
        }
        shutdown_error.map_or(Ok(()), Err)
    }
}

impl Drop for PipeChannel {
    fn drop(&mut self) {
        if let Err(error) = self.stop_reader() {
            eprintln!("failed to stop pipe reader: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::os::windows::io::FromRawHandle;
    use std::ptr::{null, null_mut};
    use std::time::{Duration, Instant};

    use windows_sys::Win32::System::Pipes::CreatePipe;

    use crate::protocol::{FrameCodec, FrameLimit};
    use crate::windows::OwnedHandle;

    use super::PipeChannel;

    #[test]
    fn stalled_read_is_cancelled_at_deadline() {
        let mut read = null_mut();
        let mut write = null_mut();
        // SAFETY: output pointers are writable and null security creates a private anonymous pipe.
        let created = unsafe { CreatePipe(&raw mut read, &raw mut write, null(), 0) };
        assert_ne!(created, 0);
        let _write = OwnedHandle::new(write).expect("own anonymous pipe writer");
        // SAFETY: ownership of the valid read handle transfers to File exactly once.
        let read = unsafe { File::from_raw_handle(read) };
        let codec = FrameCodec::new(FrameLimit::new(1024).expect("valid test frame limit"));
        let mut channel = PipeChannel::new(read, codec).expect("create test pipe channel");
        let started = Instant::now();

        let error = channel
            .receive(Duration::from_millis(25))
            .expect_err("stalled pipe must time out");

        assert!(error.to_string().contains("timed out"));
        assert!(started.elapsed() < Duration::from_secs(1));
    }
}
