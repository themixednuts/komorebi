use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::MetadataExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{RecvTimeoutError, sync_channel};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use uds_windows::{UnixListener, UnixStream};
use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

use crate::domain::FaultProfile;
use crate::installation::Layout;

const READY_LINE: &str = "READY";
const HEALTH_REQUEST: &[u8] = b"health\n";
const STOP_REQUEST: &[u8] = b"stop\n";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CandidateHealthResponse {
    pub manager_round_trip: HealthCheck,
    pub configuration_resolved: HealthCheck,
    pub windows_reconciled: HealthCheck,
    pub input_live: HealthCheck,
    pub foreign_effects_clean: HealthCheck,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthCheck {
    Passed,
    Failed,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct HealthEvidence {
    pub accepted: bool,
    pub reason: Option<HealthRejection>,
    pub elapsed_ms: f64,
    pub deadline_ms: u64,
    pub appbar_owner_count: usize,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthRejection {
    Deadline,
    CandidateExited,
    DuplicateAppBar,
    RoundTrip,
}

enum WorkerObservation {
    Ready {
        accepted: bool,
        reason: Option<HealthRejection>,
        appbar_owner_count: usize,
    },
    CandidateExited,
}

pub fn serve(layout: &Layout, fault: FaultProfile) -> Result<(), CandidateError> {
    match fault {
        FaultProfile::CandidateCrash => std::process::exit(72),
        FaultProfile::FailedIpc => loop {
            thread::park();
        },
        _ => serve_ready_candidate(layout, fault),
    }
}

pub fn probe(
    executable: &Path,
    layout: &Layout,
    fault: FaultProfile,
    deadline: Duration,
) -> Result<HealthEvidence, CandidateError> {
    let started = Instant::now();
    let mut child = Command::new(executable)
        .arg("candidate")
        .arg(layout.root())
        .arg(fault.as_str())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(CandidateError::Spawn)?;
    let Some(stdout) = child.stdout.take() else {
        terminate_child(&mut child)?;
        return Err(CandidateError::MissingStdout);
    };
    let observation_layout = layout.clone();
    let (send_observation, receive_observation) = sync_channel(1);
    let observer = thread::spawn(move || {
        let result = observe_candidate(&observation_layout, stdout);
        if send_observation.send(result).is_err() {
            // The deadline owner may have already terminated the child.
        }
    });

    let remaining = deadline.saturating_sub(started.elapsed());
    let result = match receive_observation.recv_timeout(remaining) {
        Ok(Ok(WorkerObservation::Ready {
            accepted,
            reason,
            appbar_owner_count,
        })) => {
            finish_child(&mut child)?;
            Ok(HealthEvidence {
                accepted,
                reason,
                elapsed_ms: started.elapsed().as_secs_f64() * 1_000.0,
                deadline_ms: duration_millis(deadline)?,
                appbar_owner_count,
            })
        }
        Ok(Ok(WorkerObservation::CandidateExited)) | Err(RecvTimeoutError::Disconnected) => {
            child.wait().map_err(CandidateError::Wait)?;
            Ok(HealthEvidence {
                accepted: false,
                reason: Some(HealthRejection::CandidateExited),
                elapsed_ms: started.elapsed().as_secs_f64() * 1_000.0,
                deadline_ms: duration_millis(deadline)?,
                appbar_owner_count: appbar_owner_count(layout)?,
            })
        }
        Ok(Err(error)) => {
            terminate_child(&mut child)?;
            Err(error)
        }
        Err(RecvTimeoutError::Timeout) => {
            let elapsed_ms = started.elapsed().as_secs_f64() * 1_000.0;
            terminate_child(&mut child)?;
            Ok(HealthEvidence {
                accepted: false,
                reason: Some(HealthRejection::Deadline),
                elapsed_ms,
                deadline_ms: duration_millis(deadline)?,
                appbar_owner_count: appbar_owner_count(layout)?,
            })
        }
    };

    observer
        .join()
        .map_err(|_| CandidateError::ObserverPanicked)?;
    result
}

fn observe_candidate(
    layout: &Layout,
    stdout: impl std::io::Read,
) -> Result<WorkerObservation, CandidateError> {
    let mut line = String::new();
    BufReader::new(stdout)
        .read_line(&mut line)
        .map_err(CandidateError::ReadReady)?;
    if line.trim_end() != READY_LINE {
        return Ok(WorkerObservation::CandidateExited);
    }
    let response = exchange(layout)?;
    let owners = appbar_owner_count(layout)?;
    let accepted = response.manager_round_trip == HealthCheck::Passed
        && response.configuration_resolved == HealthCheck::Passed
        && response.windows_reconciled == HealthCheck::Passed
        && response.input_live == HealthCheck::Passed
        && response.foreign_effects_clean == HealthCheck::Passed
        && owners == 1;
    let reason = if owners != 1 {
        Some(HealthRejection::DuplicateAppBar)
    } else if accepted {
        None
    } else {
        Some(HealthRejection::RoundTrip)
    };
    Ok(WorkerObservation::Ready {
        accepted,
        reason,
        appbar_owner_count: owners,
    })
}

fn serve_ready_candidate(layout: &Layout, fault: FaultProfile) -> Result<(), CandidateError> {
    fs::create_dir_all(layout.process_runtime()).map_err(CandidateError::CreateRuntime)?;
    fs::create_dir_all(layout.candidate_socket_parent()).map_err(CandidateError::CreateIpcRoot)?;
    let owner_count = if fault == FaultProfile::DuplicateAppBar {
        2
    } else {
        1
    };
    for index in 0..owner_count {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(
                layout
                    .process_runtime()
                    .join(format!("appbar-owner-{index}")),
            )
            .map_err(CandidateError::CreateOwner)?;
    }
    let listener = UnixListener::bind(layout.candidate_socket()).map_err(CandidateError::Bind)?;
    println!("{READY_LINE}");
    std::io::stdout()
        .flush()
        .map_err(CandidateError::FlushReady)?;

    let (stream, _) = listener.accept().map_err(CandidateError::Accept)?;
    let mut stream = BufReader::new(stream);
    let mut request = String::new();
    stream
        .read_line(&mut request)
        .map_err(CandidateError::ReadRequest)?;
    if request.as_bytes() != HEALTH_REQUEST {
        return Err(CandidateError::UnexpectedRequest);
    }
    let response = CandidateHealthResponse {
        manager_round_trip: HealthCheck::Passed,
        configuration_resolved: HealthCheck::Passed,
        windows_reconciled: HealthCheck::Passed,
        input_live: HealthCheck::Passed,
        foreign_effects_clean: if fault == FaultProfile::RollbackFailure {
            HealthCheck::Failed
        } else {
            HealthCheck::Passed
        },
    };
    serde_json::to_writer(&mut *stream.get_mut(), &response).map_err(CandidateError::Encode)?;
    stream
        .get_mut()
        .write_all(b"\n")
        .map_err(CandidateError::WriteResponse)?;
    stream
        .get_mut()
        .flush()
        .map_err(CandidateError::WriteResponse)?;
    request.clear();
    stream
        .read_line(&mut request)
        .map_err(CandidateError::ReadRequest)?;
    if request.as_bytes() != STOP_REQUEST {
        return Err(CandidateError::UnexpectedRequest);
    }
    Ok(())
}

fn exchange(layout: &Layout) -> Result<CandidateHealthResponse, CandidateError> {
    let mut stream =
        UnixStream::connect(layout.candidate_socket()).map_err(CandidateError::Connect)?;
    stream
        .write_all(HEALTH_REQUEST)
        .map_err(CandidateError::WriteRequest)?;
    stream.flush().map_err(CandidateError::WriteRequest)?;
    let mut stream = BufReader::new(stream);
    let mut response = String::new();
    stream
        .read_line(&mut response)
        .map_err(CandidateError::ReadResponse)?;
    let response = serde_json::from_str(&response).map_err(CandidateError::Decode)?;
    stream
        .get_mut()
        .write_all(STOP_REQUEST)
        .map_err(CandidateError::WriteRequest)?;
    stream
        .get_mut()
        .flush()
        .map_err(CandidateError::WriteRequest)?;
    Ok(response)
}

fn finish_child(child: &mut Child) -> Result<(), CandidateError> {
    let status = child.wait().map_err(CandidateError::Wait)?;
    if status.success() {
        Ok(())
    } else {
        Err(CandidateError::Unsuccessful(status.code()))
    }
}

fn terminate_child(child: &mut Child) -> Result<(), CandidateError> {
    child.kill().map_err(CandidateError::Kill)?;
    child.wait().map_err(CandidateError::Wait)?;
    Ok(())
}

fn appbar_owner_count(layout: &Layout) -> Result<usize, CandidateError> {
    let mut count = 0;
    for entry in fs::read_dir(layout.process_runtime()).map_err(CandidateError::ReadRuntime)? {
        let entry = entry.map_err(CandidateError::ReadRuntime)?;
        if starts_with_ascii(&entry.file_name(), b"appbar-owner-") {
            let metadata = entry.metadata().map_err(CandidateError::ReadMetadata)?;
            if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                return Err(CandidateError::ReparsePoint(entry.path()));
            }
            count += 1;
        }
    }
    Ok(count)
}

fn starts_with_ascii(value: &std::ffi::OsStr, prefix: &[u8]) -> bool {
    let mut units = value.encode_wide();
    prefix
        .iter()
        .copied()
        .map(u16::from)
        .all(|expected| units.next() == Some(expected))
}

fn duration_millis(duration: Duration) -> Result<u64, CandidateError> {
    u64::try_from(duration.as_millis()).map_err(|_| CandidateError::DeadlineRange)
}

#[derive(Debug, Error)]
pub enum CandidateError {
    #[error("spawn candidate process")]
    Spawn(#[source] std::io::Error),
    #[error("candidate stdout pipe is missing")]
    MissingStdout,
    #[error("candidate health observer panicked")]
    ObserverPanicked,
    #[error("read candidate ready event")]
    ReadReady(#[source] std::io::Error),
    #[error("candidate deadline does not fit u64 milliseconds")]
    DeadlineRange,
    #[error("kill candidate after health deadline")]
    Kill(#[source] std::io::Error),
    #[error("wait for candidate process")]
    Wait(#[source] std::io::Error),
    #[error("candidate exited unsuccessfully with code {0:?}")]
    Unsuccessful(Option<i32>),
    #[error("create candidate runtime")]
    CreateRuntime(#[source] std::io::Error),
    #[error("create candidate IPC directory")]
    CreateIpcRoot(#[source] std::io::Error),
    #[error("create AppBar owner marker")]
    CreateOwner(#[source] std::io::Error),
    #[error("bind candidate IPC")]
    Bind(#[source] std::io::Error),
    #[error("flush candidate ready event")]
    FlushReady(#[source] std::io::Error),
    #[error("accept candidate IPC")]
    Accept(#[source] std::io::Error),
    #[error("connect candidate IPC")]
    Connect(#[source] std::io::Error),
    #[error("write candidate request")]
    WriteRequest(#[source] std::io::Error),
    #[error("read candidate request")]
    ReadRequest(#[source] std::io::Error),
    #[error("unexpected candidate request")]
    UnexpectedRequest,
    #[error("encode candidate response")]
    Encode(#[source] serde_json::Error),
    #[error("write candidate response")]
    WriteResponse(#[source] std::io::Error),
    #[error("read candidate response")]
    ReadResponse(#[source] std::io::Error),
    #[error("decode candidate response")]
    Decode(#[source] serde_json::Error),
    #[error("read candidate runtime")]
    ReadRuntime(#[source] std::io::Error),
    #[error("read candidate runtime entry metadata")]
    ReadMetadata(#[source] std::io::Error),
    #[error("candidate runtime entry is a reparse point: {0:?}")]
    ReparsePoint(std::path::PathBuf),
}
