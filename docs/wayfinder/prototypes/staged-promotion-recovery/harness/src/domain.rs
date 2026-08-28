use std::fmt::{Display, Formatter};
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct InstallationId(String);

impl InstallationId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for InstallationId {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for InstallationId {
    type Err = InvalidInstallationId;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let valid = !value.is_empty()
            && value.len() <= 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
        valid
            .then(|| Self(value.to_owned()))
            .ok_or(InvalidInstallationId)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FaultProfile {
    Healthy,
    InvalidConfiguration,
    FailedIpc,
    DuplicateAppBar,
    CandidateCrash,
    RollbackFailure,
}

impl FaultProfile {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::InvalidConfiguration => "invalid_configuration",
            Self::FailedIpc => "failed_ipc",
            Self::DuplicateAppBar => "duplicate_app_bar",
            Self::CandidateCrash => "candidate_crash",
            Self::RollbackFailure => "rollback_failure",
        }
    }
}

impl FromStr for FaultProfile {
    type Err = InvalidFaultProfile;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "healthy" => Ok(Self::Healthy),
            "invalid_configuration" => Ok(Self::InvalidConfiguration),
            "failed_ipc" => Ok(Self::FailedIpc),
            "duplicate_app_bar" => Ok(Self::DuplicateAppBar),
            "candidate_crash" => Ok(Self::CandidateCrash),
            "rollback_failure" => Ok(Self::RollbackFailure),
            _ => Err(InvalidFaultProfile),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Boundary {
    Prepared,
    CandidateSealed,
    ConfigurationMigrated,
    WindowSnapshotCaptured,
    InputPaused,
    OwnedShellStopped,
    ActivePointerSwitched,
    CandidateStarted,
    WindowsReconciled,
    InputAndUiStarted,
    HealthAccepted,
    PromotionCommitted,
    HealthRejected,
    RollbackStarted,
    PriorPointerRestored,
    PriorStarted,
    RollbackHealthAccepted,
    RollbackCompleted,
    RollbackRejected,
    SafeStopStarted,
    EffectsRestored,
    ProcessesStopped,
    SafeStopCompleted,
}

impl Boundary {
    pub const PROMOTION: [Self; 12] = [
        Self::Prepared,
        Self::CandidateSealed,
        Self::ConfigurationMigrated,
        Self::WindowSnapshotCaptured,
        Self::InputPaused,
        Self::OwnedShellStopped,
        Self::ActivePointerSwitched,
        Self::CandidateStarted,
        Self::WindowsReconciled,
        Self::InputAndUiStarted,
        Self::HealthAccepted,
        Self::PromotionCommitted,
    ];

    pub const ROLLBACK: [Self; 6] = [
        Self::HealthRejected,
        Self::RollbackStarted,
        Self::PriorPointerRestored,
        Self::PriorStarted,
        Self::RollbackHealthAccepted,
        Self::RollbackCompleted,
    ];

    pub const SAFE_STOP: [Self; 7] = [
        Self::HealthRejected,
        Self::RollbackStarted,
        Self::RollbackRejected,
        Self::SafeStopStarted,
        Self::EffectsRestored,
        Self::ProcessesStopped,
        Self::SafeStopCompleted,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Prepared => "prepared",
            Self::CandidateSealed => "candidate_sealed",
            Self::ConfigurationMigrated => "configuration_migrated",
            Self::WindowSnapshotCaptured => "window_snapshot_captured",
            Self::InputPaused => "input_paused",
            Self::OwnedShellStopped => "owned_shell_stopped",
            Self::ActivePointerSwitched => "active_pointer_switched",
            Self::CandidateStarted => "candidate_started",
            Self::WindowsReconciled => "windows_reconciled",
            Self::InputAndUiStarted => "input_and_ui_started",
            Self::HealthAccepted => "health_accepted",
            Self::PromotionCommitted => "promotion_committed",
            Self::HealthRejected => "health_rejected",
            Self::RollbackStarted => "rollback_started",
            Self::PriorPointerRestored => "prior_pointer_restored",
            Self::PriorStarted => "prior_started",
            Self::RollbackHealthAccepted => "rollback_health_accepted",
            Self::RollbackCompleted => "rollback_completed",
            Self::RollbackRejected => "rollback_rejected",
            Self::SafeStopStarted => "safe_stop_started",
            Self::EffectsRestored => "effects_restored",
            Self::ProcessesStopped => "processes_stopped",
            Self::SafeStopCompleted => "safe_stop_completed",
        }
    }
}

impl Display for Boundary {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for Boundary {
    type Err = InvalidBoundary;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "prepared" => Ok(Self::Prepared),
            "candidate_sealed" => Ok(Self::CandidateSealed),
            "configuration_migrated" => Ok(Self::ConfigurationMigrated),
            "window_snapshot_captured" => Ok(Self::WindowSnapshotCaptured),
            "input_paused" => Ok(Self::InputPaused),
            "owned_shell_stopped" => Ok(Self::OwnedShellStopped),
            "active_pointer_switched" => Ok(Self::ActivePointerSwitched),
            "candidate_started" => Ok(Self::CandidateStarted),
            "windows_reconciled" => Ok(Self::WindowsReconciled),
            "input_and_ui_started" => Ok(Self::InputAndUiStarted),
            "health_accepted" => Ok(Self::HealthAccepted),
            "promotion_committed" => Ok(Self::PromotionCommitted),
            "health_rejected" => Ok(Self::HealthRejected),
            "rollback_started" => Ok(Self::RollbackStarted),
            "prior_pointer_restored" => Ok(Self::PriorPointerRestored),
            "prior_started" => Ok(Self::PriorStarted),
            "rollback_health_accepted" => Ok(Self::RollbackHealthAccepted),
            "rollback_completed" => Ok(Self::RollbackCompleted),
            "rollback_rejected" => Ok(Self::RollbackRejected),
            "safe_stop_started" => Ok(Self::SafeStopStarted),
            "effects_restored" => Ok(Self::EffectsRestored),
            "processes_stopped" => Ok(Self::ProcessesStopped),
            "safe_stop_completed" => Ok(Self::SafeStopCompleted),
            _ => Err(InvalidBoundary),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PromotionIdentity {
    pub transaction: String,
    pub prior: InstallationId,
    pub candidate: InstallationId,
    pub fault: FaultProfile,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Convergence {
    Candidate,
    Prior,
    SafeStopped,
    StagingRejected,
}

#[derive(Debug, Error, Eq, PartialEq)]
#[error("installation identifier must contain 1 to 64 lowercase ASCII letters, digits, or hyphens")]
pub struct InvalidInstallationId;

#[derive(Debug, Error, Eq, PartialEq)]
#[error("unknown fault profile")]
pub struct InvalidFaultProfile;

#[derive(Debug, Error, Eq, PartialEq)]
#[error("unknown journal boundary")]
pub struct InvalidBoundary;

#[cfg(test)]
mod tests {
    use super::{Boundary, InstallationId};

    #[test]
    fn installation_id_rejects_path_syntax() {
        for invalid in ["", "../candidate", "C:active", "active/config", "ACTIVE"] {
            assert!(invalid.parse::<InstallationId>().is_err(), "{invalid}");
        }
    }

    #[test]
    fn every_boundary_round_trips_through_its_protocol_name() {
        for boundary in Boundary::PROMOTION
            .into_iter()
            .chain(Boundary::ROLLBACK)
            .chain(Boundary::SAFE_STOP)
        {
            assert_eq!(boundary.to_string().parse(), Ok(boundary));
        }
    }
}
