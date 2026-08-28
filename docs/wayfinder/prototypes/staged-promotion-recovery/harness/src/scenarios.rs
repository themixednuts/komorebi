use std::ffi::OsString;
use std::fs::OpenOptions;
use std::io::Write;
use std::os::windows::ffi::OsStringExt;
use std::path::Path;
use std::process::{Command, Output};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tempfile::TempDir;
use thiserror::Error;

use crate::candidate::{HealthEvidence, HealthRejection};
use crate::domain::{Boundary, Convergence, FaultProfile};
use crate::promotion::PromotionOutcome;

const PROCESS_DEATH_EXIT_CODE: i32 = 86;

#[derive(Debug, Deserialize, Serialize)]
pub struct MeasurementReport {
    pub schema_version: u8,
    pub health_deadline_ms: u64,
    pub scenario_count: usize,
    pub process_death_boundary_count: usize,
    pub all_passed: bool,
    pub scenarios: Vec<ScenarioEvidence>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ScenarioEvidence {
    pub name: String,
    pub fault: FaultProfile,
    pub crash_after: Option<Boundary>,
    pub expected: Convergence,
    pub observed: Convergence,
    pub elapsed_ms: f64,
    pub recovery_ms: Option<f64>,
    pub health: Option<HealthEvidence>,
    pub passed: bool,
}

pub fn run_all(
    executable: &Path,
    output_path: &Path,
    health_deadline: Duration,
) -> Result<MeasurementReport, ScenarioError> {
    let mut scenarios = Vec::new();
    for (name, fault, expected) in [
        (
            "healthy promotion",
            FaultProfile::Healthy,
            Convergence::Candidate,
        ),
        (
            "invalid configuration",
            FaultProfile::InvalidConfiguration,
            Convergence::StagingRejected,
        ),
        (
            "failed IPC reaches exact health deadline",
            FaultProfile::FailedIpc,
            Convergence::Prior,
        ),
        (
            "duplicate AppBar owner",
            FaultProfile::DuplicateAppBar,
            Convergence::Prior,
        ),
        (
            "candidate process crash",
            FaultProfile::CandidateCrash,
            Convergence::Prior,
        ),
        (
            "rollback failure enters safe stop",
            FaultProfile::RollbackFailure,
            Convergence::SafeStopped,
        ),
    ] {
        scenarios.push(run_scenario(
            executable,
            name,
            fault,
            None,
            expected,
            health_deadline,
        )?);
    }

    scenarios.push(run_wtf16_path_scenario(executable, health_deadline)?);

    for boundary in Boundary::PROMOTION {
        let expected = if boundary == Boundary::PromotionCommitted {
            Convergence::Candidate
        } else {
            Convergence::Prior
        };
        scenarios.push(run_scenario(
            executable,
            &format!("process death after {boundary}"),
            FaultProfile::Healthy,
            Some(boundary),
            expected,
            health_deadline,
        )?);
    }
    for boundary in Boundary::ROLLBACK {
        scenarios.push(run_scenario(
            executable,
            &format!("process death after {boundary}"),
            FaultProfile::DuplicateAppBar,
            Some(boundary),
            Convergence::Prior,
            health_deadline,
        )?);
    }
    for boundary in Boundary::SAFE_STOP {
        scenarios.push(run_scenario(
            executable,
            &format!("process death after {boundary}"),
            FaultProfile::RollbackFailure,
            Some(boundary),
            Convergence::SafeStopped,
            health_deadline,
        )?);
    }

    let process_death_boundary_count = scenarios
        .iter()
        .filter(|scenario| scenario.crash_after.is_some())
        .count();
    let report = MeasurementReport {
        schema_version: 1,
        health_deadline_ms: duration_millis(health_deadline)?,
        scenario_count: scenarios.len(),
        process_death_boundary_count,
        all_passed: scenarios.iter().all(|scenario| scenario.passed),
        scenarios,
    };
    write_new_json(output_path, &report)?;
    if report.all_passed {
        Ok(report)
    } else {
        Err(ScenarioError::ScenarioMismatch)
    }
}

fn run_wtf16_path_scenario(
    executable: &Path,
    deadline: Duration,
) -> Result<ScenarioEvidence, ScenarioError> {
    let parent = TempDir::new().map_err(ScenarioError::CreateTemporaryRoot)?;
    let mut units = "promotion-".encode_utf16().collect::<Vec<_>>();
    units.push(0xD800);
    let root = parent.path().join(OsString::from_wide(&units));
    std::fs::create_dir(&root).map_err(ScenarioError::CreateWtf16Root)?;
    run_scenario_at(
        executable,
        "unpaired UTF-16 path survives process boundary",
        FaultProfile::Healthy,
        None,
        Convergence::Candidate,
        deadline,
        &root,
    )
}

fn run_scenario(
    executable: &Path,
    name: &str,
    fault: FaultProfile,
    crash_after: Option<Boundary>,
    expected: Convergence,
    deadline: Duration,
) -> Result<ScenarioEvidence, ScenarioError> {
    let root = TempDir::new().map_err(ScenarioError::CreateTemporaryRoot)?;
    run_scenario_at(
        executable,
        name,
        fault,
        crash_after,
        expected,
        deadline,
        root.path(),
    )
}

fn run_scenario_at(
    executable: &Path,
    name: &str,
    fault: FaultProfile,
    crash_after: Option<Boundary>,
    expected: Convergence,
    deadline: Duration,
    root: &Path,
) -> Result<ScenarioEvidence, ScenarioError> {
    let started = Instant::now();
    let mut command = Command::new(executable);
    command
        .arg("attempt")
        .arg(root)
        .arg(fault.as_str())
        .arg(duration_millis(deadline)?.to_string());
    if let Some(boundary) = crash_after {
        command.arg(boundary.to_string());
    }
    let attempt = command.output().map_err(ScenarioError::SpawnAttempt)?;

    let (outcome, recovery_ms) = if crash_after.is_some() {
        if attempt.status.code() != Some(PROCESS_DEATH_EXIT_CODE) {
            return Err(ScenarioError::ExpectedProcessDeath {
                name: name.to_owned(),
                output: ProcessOutput::from(attempt),
            });
        }
        let recovery_started = Instant::now();
        let recovery = Command::new(executable)
            .arg("recover")
            .arg(root)
            .output()
            .map_err(ScenarioError::SpawnRecovery)?;
        let elapsed = recovery_started.elapsed().as_secs_f64() * 1_000.0;
        (decode_success(name, recovery)?, Some(elapsed))
    } else {
        (decode_success(name, attempt)?, None)
    };
    let observed = outcome.convergence;
    let health = outcome.health;
    let deadline_passed = if fault == FaultProfile::FailedIpc && crash_after.is_none() {
        health.as_ref().is_some_and(|evidence| {
            let Ok(deadline_ms) = u32::try_from(evidence.deadline_ms).map(f64::from) else {
                return false;
            };
            evidence.reason == Some(HealthRejection::Deadline)
                && evidence.elapsed_ms >= deadline_ms
                && evidence.elapsed_ms <= deadline_ms + 50.0
        })
    } else {
        true
    };
    Ok(ScenarioEvidence {
        name: name.to_owned(),
        fault,
        crash_after,
        expected,
        observed,
        elapsed_ms: started.elapsed().as_secs_f64() * 1_000.0,
        recovery_ms,
        health,
        passed: observed == expected && deadline_passed,
    })
}

fn decode_success(name: &str, output: Output) -> Result<PromotionOutcome, ScenarioError> {
    if !output.status.success() {
        return Err(ScenarioError::CommandFailed {
            name: name.to_owned(),
            output: ProcessOutput::from(output),
        });
    }
    serde_json::from_slice(&output.stdout).map_err(ScenarioError::DecodeOutcome)
}

fn write_new_json(path: &Path, value: &impl Serialize) -> Result<(), ScenarioError> {
    let bytes = serde_json::to_vec_pretty(value).map_err(ScenarioError::EncodeReport)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(ScenarioError::CreateReport)?;
    file.write_all(&bytes).map_err(ScenarioError::WriteReport)?;
    file.write_all(b"\n").map_err(ScenarioError::WriteReport)?;
    file.sync_all().map_err(ScenarioError::SyncReport)
}

fn duration_millis(duration: Duration) -> Result<u64, ScenarioError> {
    u64::try_from(duration.as_millis()).map_err(|_| ScenarioError::DeadlineRange)
}

#[derive(Debug)]
pub struct ProcessOutput {
    status: Option<i32>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

impl From<Output> for ProcessOutput {
    fn from(output: Output) -> Self {
        Self {
            status: output.status.code(),
            stdout: output.stdout,
            stderr: output.stderr,
        }
    }
}

impl std::fmt::Display for ProcessOutput {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "status={:?}, stdout={:?}, stderr={:?}",
            self.status, self.stdout, self.stderr
        )
    }
}

#[derive(Debug, Error)]
pub enum ScenarioError {
    #[error("create temporary scenario root")]
    CreateTemporaryRoot(#[source] std::io::Error),
    #[error("create WTF-16 scenario root")]
    CreateWtf16Root(#[source] std::io::Error),
    #[error("health deadline does not fit u64 milliseconds")]
    DeadlineRange,
    #[error("spawn promotion attempt")]
    SpawnAttempt(#[source] std::io::Error),
    #[error("spawn promotion recovery")]
    SpawnRecovery(#[source] std::io::Error),
    #[error("scenario {name} did not reach the requested process-death boundary: {output}")]
    ExpectedProcessDeath { name: String, output: ProcessOutput },
    #[error("scenario {name} command failed: {output}")]
    CommandFailed { name: String, output: ProcessOutput },
    #[error("decode promotion outcome")]
    DecodeOutcome(#[source] serde_json::Error),
    #[error("encode measurement report")]
    EncodeReport(#[source] serde_json::Error),
    #[error("create new measurement report")]
    CreateReport(#[source] std::io::Error),
    #[error("write measurement report")]
    WriteReport(#[source] std::io::Error),
    #[error("sync measurement report")]
    SyncReport(#[source] std::io::Error),
    #[error("one or more measurement scenarios did not converge as expected")]
    ScenarioMismatch,
}
