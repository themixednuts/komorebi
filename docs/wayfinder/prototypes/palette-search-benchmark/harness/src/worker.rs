use std::num::NonZeroU64;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::io::{BufReader, BufWriter};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use windows::Win32::System::Threading::CREATE_NO_WINDOW;

use crate::fff::FileSnapshot;
use crate::job::{JobError, KillOnCloseJob};
use crate::native::{
    NativeError, ProcessCounters, ProcessMemory, process_counters, process_memory,
};
use crate::protocol::{
    ProtocolError, WorkerFailure, WorkerRequest, WorkerRequestEnvelope, WorkerResponse,
    WorkerWireResponse, read_frame, write_frame,
};

pub async fn run_worker() -> Result<(), WorkerError> {
    let mut input = BufReader::new(tokio::io::stdin());
    let mut output = BufWriter::new(tokio::io::stdout());
    write_frame(
        &mut output,
        &WorkerWireResponse::Ready {
            process_id: std::process::id(),
        },
    )
    .await?;
    let mut snapshot = None;
    loop {
        let envelope: WorkerRequestEnvelope = read_frame(&mut input).await?;
        let request_id = envelope.request_id;
        match envelope.request {
            WorkerRequest::Build { root } => {
                let path = root.into_path();
                let result = tokio::task::spawn_blocking(move || FileSnapshot::build(&path))
                    .await
                    .map_err(|_| WorkerError::BlockingTask)?;
                match result {
                    Ok((built, measurement)) => {
                        snapshot = Some(built);
                        write_reply(&mut output, request_id, WorkerResponse::Built(measurement))
                            .await?;
                    }
                    Err(_) => {
                        write_reply(
                            &mut output,
                            request_id,
                            WorkerResponse::Rejected(WorkerFailure::Dependency),
                        )
                        .await?;
                    }
                }
            }
            WorkerRequest::SearchName {
                fence,
                query,
                limit,
            } => {
                let Some(current) = snapshot.take() else {
                    write_snapshot_missing(&mut output, request_id).await?;
                    continue;
                };
                let (current, response) = tokio::task::spawn_blocking(move || {
                    let response = match current.search_name(&query, limit) {
                        Ok(measurement) => WorkerResponse::Name { fence, measurement },
                        Err(_) => WorkerResponse::Rejected(WorkerFailure::Dependency),
                    };
                    (current, response)
                })
                .await
                .map_err(|_| WorkerError::BlockingTask)?;
                snapshot = Some(current);
                write_reply(&mut output, request_id, response).await?;
            }
            WorkerRequest::SearchContent {
                fence,
                query,
                limits,
            } => {
                let Some(current) = snapshot.take() else {
                    write_snapshot_missing(&mut output, request_id).await?;
                    continue;
                };
                let (current, response) = tokio::task::spawn_blocking(move || {
                    let response = match current.search_content(
                        &query,
                        limits,
                        &Arc::new(AtomicBool::new(false)),
                    ) {
                        Ok(measurement) => WorkerResponse::Content { fence, measurement },
                        Err(_) => WorkerResponse::Rejected(WorkerFailure::Dependency),
                    };
                    (current, response)
                })
                .await
                .map_err(|_| WorkerError::BlockingTask)?;
                snapshot = Some(current);
                write_reply(&mut output, request_id, response).await?;
            }
            WorkerRequest::Crash => std::process::abort(),
            WorkerRequest::Hang => std::future::pending::<()>().await,
            WorkerRequest::Shutdown => {
                write_reply(&mut output, request_id, WorkerResponse::Shutdown).await?;
                return Ok(());
            }
        }
    }
}

async fn write_snapshot_missing<W>(
    output: &mut W,
    request_id: crate::domain::RequestId,
) -> Result<(), WorkerError>
where
    W: tokio::io::AsyncWrite + Unpin,
{
    write_reply(
        output,
        request_id,
        WorkerResponse::Rejected(WorkerFailure::SnapshotMissing),
    )
    .await
}

async fn write_reply<W>(
    output: &mut W,
    request_id: crate::domain::RequestId,
    response: WorkerResponse,
) -> Result<(), WorkerError>
where
    W: tokio::io::AsyncWrite + Unpin,
{
    write_frame(
        output,
        &WorkerWireResponse::Reply {
            request_id,
            response,
        },
    )
    .await?;
    Ok(())
}

pub struct WorkerClient {
    job: KillOnCloseJob,
    child: Child,
    input: BufWriter<ChildStdin>,
    output: BufReader<ChildStdout>,
    next_request: NonZeroU64,
}

impl WorkerClient {
    pub async fn spawn(executable: &Path) -> Result<(Self, WorkerStartupMeasurement), WorkerError> {
        let started = Instant::now();
        let job = KillOnCloseJob::create()?;
        let mut child = Command::new(executable)
            .arg("worker")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW.0)
            .kill_on_drop(true)
            .spawn()?;
        let process_handle = child.raw_handle().ok_or(WorkerError::ProcessHandle)?;
        job.assign(windows::Win32::Foundation::HANDLE(process_handle))?;
        let input = child.stdin.take().ok_or(WorkerError::MissingPipe)?;
        let output = child.stdout.take().ok_or(WorkerError::MissingPipe)?;
        let mut client = Self {
            job,
            child,
            input: BufWriter::new(input),
            output: BufReader::new(output),
            next_request: NonZeroU64::MIN,
        };
        let response = client.receive(Duration::from_secs(5)).await?;
        let WorkerWireResponse::Ready { process_id } = response else {
            return Err(WorkerError::Handshake);
        };
        if client.child.id() != Some(process_id) {
            return Err(WorkerError::Handshake);
        }
        Ok((
            client,
            WorkerStartupMeasurement {
                startup_ns: nanos(started.elapsed())?,
                process_id_verified: true,
            },
        ))
    }

    pub async fn request(
        &mut self,
        request: &WorkerRequest,
        deadline: Duration,
    ) -> Result<WorkerResponse, WorkerError> {
        let request_id = crate::domain::RequestId::new(self.next_request);
        self.next_request = self
            .next_request
            .get()
            .checked_add(1)
            .and_then(NonZeroU64::new)
            .ok_or(WorkerError::RequestIdExhausted)?;
        write_frame(
            &mut self.input,
            &WorkerRequestEnvelope {
                request_id,
                request: request.clone(),
            },
        )
        .await?;
        match self.receive(deadline).await? {
            WorkerWireResponse::Reply {
                request_id: response_id,
                response,
            } if response_id.value() == request_id.value() => Ok(response),
            _ => Err(WorkerError::ResponseCorrelation),
        }
    }

    pub fn counters(&self) -> Result<ProcessCounters, WorkerError> {
        let handle = self.child.raw_handle().ok_or(WorkerError::ProcessHandle)?;
        Ok(process_counters(windows::Win32::Foundation::HANDLE(
            handle,
        ))?)
    }

    pub fn memory(&self) -> Result<ProcessMemory, WorkerError> {
        let handle = self.child.raw_handle().ok_or(WorkerError::ProcessHandle)?;
        Ok(process_memory(windows::Win32::Foundation::HANDLE(handle))?)
    }

    pub async fn terminate_and_wait(mut self) -> Result<WorkerExitMeasurement, WorkerError> {
        let started = Instant::now();
        self.job.terminate(0xE043_0001)?;
        let status = self.child.wait().await?;
        Ok(WorkerExitMeasurement {
            elapsed_ns: nanos(started.elapsed())?,
            exited: true,
            success: status.success(),
        })
    }

    async fn receive(&mut self, deadline: Duration) -> Result<WorkerWireResponse, WorkerError> {
        tokio::time::timeout(deadline, read_frame(&mut self.output))
            .await
            .map_err(|_| WorkerError::Deadline)?
            .map_err(WorkerError::Protocol)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct WorkerStartupMeasurement {
    pub startup_ns: u64,
    pub process_id_verified: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct WorkerExitMeasurement {
    pub elapsed_ns: u64,
    pub exited: bool,
    pub success: bool,
}

fn nanos(duration: Duration) -> Result<u64, WorkerError> {
    u64::try_from(duration.as_nanos()).map_err(|_| WorkerError::DurationOverflow)
}

#[derive(Debug, Error)]
pub enum WorkerError {
    #[error("worker transport protocol failed")]
    Protocol(#[from] ProtocolError),
    #[error("worker process operation failed")]
    Io(#[from] std::io::Error),
    #[error("worker Job Object operation failed")]
    Job(#[from] JobError),
    #[error("worker native process inspection failed")]
    Native(#[from] NativeError),
    #[error("worker did not expose a process handle")]
    ProcessHandle,
    #[error("worker stdio contract is incomplete")]
    MissingPipe,
    #[error("worker handshake identity is invalid")]
    Handshake,
    #[error("worker missed its request deadline")]
    Deadline,
    #[error("worker blocking actor failed")]
    BlockingTask,
    #[error("duration does not fit the report representation")]
    DurationOverflow,
    #[error("worker request identity space was exhausted")]
    RequestIdExhausted,
    #[error("worker response did not match its request identity")]
    ResponseCorrelation,
}
