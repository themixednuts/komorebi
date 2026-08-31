use std::fs::File;
use std::io;

use thiserror::Error;
use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
use windows_sys::Win32::Foundation::WAIT_TIMEOUT;
use windows_sys::Win32::System::Threading::GetExitCodeProcess;
use windows_sys::Win32::System::Threading::WaitForSingleObject;

use super::resources::OwnedHandle;
use super::resources::WorkerJob;
use super::resources::terminate;
use crate::PluginId;
use crate::PluginLimits;
use crate::PluginLoadFailure;
use crate::PluginLoadReport;
use crate::PluginManifest;
use crate::PluginProgram;
use crate::wire;
use crate::wire::Request;
use crate::wire::Response;
use crate::wire::WireError;

const SHUTDOWN_TIMEOUT_MILLIS: u32 = 15_000;

pub(crate) struct NativeWorkerSession {
    process: OwnedHandle,
    _job: WorkerJob,
    reader: File,
    writer: File,
    plugin: PluginId,
}

impl NativeWorkerSession {
    pub(super) const fn new(
        process: OwnedHandle,
        job: WorkerJob,
        reader: File,
        writer: File,
        plugin: PluginId,
    ) -> Self {
        Self {
            process,
            _job: job,
            reader,
            writer,
            plugin,
        }
    }

    pub(crate) fn await_ready(&mut self) -> Result<(), LpacSessionError> {
        match wire::read_response(&mut self.reader, &self.plugin)? {
            Response::Ready => Ok(()),
            _ => Err(LpacSessionError::UnexpectedResponse),
        }
    }

    pub(crate) fn initialize(
        &mut self,
        manifest: PluginManifest,
        limits: PluginLimits,
        program: PluginProgram,
    ) -> Result<PluginLoadReport, LpacSessionError> {
        self.exchange(&Request::Initialize {
            manifest,
            limits,
            program,
        })
    }

    pub(crate) fn reload(
        &mut self,
        program: PluginProgram,
    ) -> Result<PluginLoadReport, LpacSessionError> {
        self.exchange(&Request::Reload(program))
    }

    pub(crate) fn shutdown(mut self) -> Result<(), LpacSessionError> {
        wire::write_request(&mut self.writer, &Request::Shutdown)?;
        match wire::read_response(&mut self.reader, &self.plugin)? {
            Response::Stopped => {}
            _ => return Err(LpacSessionError::UnexpectedResponse),
        }
        let wait = unsafe {
            // SAFETY: the session uniquely owns this live process handle.
            WaitForSingleObject(self.process.handle(), SHUTDOWN_TIMEOUT_MILLIS)
        };
        if wait == WAIT_TIMEOUT {
            terminate(self.process.handle());
            return Err(LpacSessionError::ShutdownTimeout);
        }
        if wait != WAIT_OBJECT_0 {
            return Err(LpacSessionError::windows("WaitForSingleObject"));
        }
        let mut exit_code = 0;
        if unsafe {
            // SAFETY: the process has signalled and output storage is writable.
            GetExitCodeProcess(self.process.handle(), &raw mut exit_code)
        } == 0
        {
            return Err(LpacSessionError::windows("GetExitCodeProcess"));
        }
        if exit_code == 0 {
            Ok(())
        } else {
            Err(LpacSessionError::WorkerExited(exit_code))
        }
    }

    fn exchange(&mut self, request: &Request) -> Result<PluginLoadReport, LpacSessionError> {
        wire::write_request(&mut self.writer, request)?;
        match wire::read_response(&mut self.reader, &self.plugin)? {
            Response::Loaded(report) => Ok(report),
            Response::Rejected(failure) => Err(LpacSessionError::Rejected(failure)),
            _ => Err(LpacSessionError::UnexpectedResponse),
        }
    }
}

#[derive(Debug, Error)]
pub enum LpacSessionError {
    #[error(transparent)]
    Launch(#[from] super::LpacLaunchError),
    #[error(transparent)]
    Wire(#[from] WireError),
    #[error(transparent)]
    Rejected(#[from] PluginLoadFailure),
    #[error("LPAC worker returned an unexpected protocol response")]
    UnexpectedResponse,
    #[error("LPAC worker did not stop within its bounded shutdown window")]
    ShutdownTimeout,
    #[error("LPAC worker exited with code {0}")]
    WorkerExited(u32),
    #[error("{operation} failed: {source}")]
    Windows {
        operation: &'static str,
        #[source]
        source: io::Error,
    },
}

impl LpacSessionError {
    fn windows(operation: &'static str) -> Self {
        Self::Windows {
            operation,
            source: io::Error::last_os_error(),
        }
    }
}
