use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::candidate::{self, HealthEvidence};
use crate::domain::{Boundary, Convergence, FaultProfile, PromotionIdentity};
use crate::installation::{InstallError, Layout, RuntimeState};
use crate::journal::{Journal, JournalError};

#[derive(Clone, Copy, Debug, Default)]
pub struct CrashAfter(Option<Boundary>);

impl CrashAfter {
    pub const fn never() -> Self {
        Self(None)
    }

    pub const fn boundary(boundary: Boundary) -> Self {
        Self(Some(boundary))
    }

    fn reached(self, boundary: Boundary) -> Result<(), PromotionError> {
        if self.0 == Some(boundary) {
            Err(PromotionError::InjectedProcessDeath(boundary))
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PromotionOutcome {
    pub convergence: Convergence,
    pub terminal_boundary: Boundary,
    pub health: Option<HealthEvidence>,
}

pub fn attempt(
    executable: &Path,
    layout: &Layout,
    identity: &PromotionIdentity,
    deadline: Duration,
    crash_after: CrashAfter,
) -> Result<PromotionOutcome, PromotionError> {
    let mut journal = Journal::open(&layout.store_path())?;
    append(&mut journal, identity, Boundary::Prepared, crash_after)?;

    layout.seal_candidate(identity)?;
    append(
        &mut journal,
        identity,
        Boundary::CandidateSealed,
        crash_after,
    )?;

    if let Err(error) = layout.migrate_configuration(identity) {
        if matches!(error, InstallError::InvalidConfiguration(_)) {
            layout.verify_convergence(Convergence::StagingRejected, identity)?;
            return Ok(PromotionOutcome {
                convergence: Convergence::StagingRejected,
                terminal_boundary: Boundary::CandidateSealed,
                health: None,
            });
        }
        return Err(error.into());
    }
    append(
        &mut journal,
        identity,
        Boundary::ConfigurationMigrated,
        crash_after,
    )?;

    layout.capture_windows(identity)?;
    append(
        &mut journal,
        identity,
        Boundary::WindowSnapshotCaptured,
        crash_after,
    )?;
    layout.set_input(RuntimeState::Stopped)?;
    append(&mut journal, identity, Boundary::InputPaused, crash_after)?;
    layout.set_shell(RuntimeState::Stopped)?;
    append(
        &mut journal,
        identity,
        Boundary::OwnedShellStopped,
        crash_after,
    )?;
    layout.switch_active(&identity.candidate)?;
    append(
        &mut journal,
        identity,
        Boundary::ActivePointerSwitched,
        crash_after,
    )?;
    layout.mark_candidate_started()?;
    append(
        &mut journal,
        identity,
        Boundary::CandidateStarted,
        crash_after,
    )?;
    layout.verify_candidate_seal(identity)?;
    layout.verify_windows_and_appearance(identity)?;
    append(
        &mut journal,
        identity,
        Boundary::WindowsReconciled,
        crash_after,
    )?;
    layout.set_input(RuntimeState::Live)?;
    layout.set_shell(RuntimeState::Live)?;
    append(
        &mut journal,
        identity,
        Boundary::InputAndUiStarted,
        crash_after,
    )?;

    let health = candidate::probe(executable, layout, identity.fault, deadline)?;
    if health.accepted {
        append(
            &mut journal,
            identity,
            Boundary::HealthAccepted,
            crash_after,
        )?;
        append(
            &mut journal,
            identity,
            Boundary::PromotionCommitted,
            crash_after,
        )?;
        layout.cleanup_snapshot(identity)?;
        layout.verify_convergence(Convergence::Candidate, identity)?;
        Ok(PromotionOutcome {
            convergence: Convergence::Candidate,
            terminal_boundary: Boundary::PromotionCommitted,
            health: Some(health),
        })
    } else {
        converge_prior(layout, identity, &mut journal, crash_after, Some(health))
    }
}

pub fn recover(
    layout: &Layout,
    crash_after: CrashAfter,
) -> Result<PromotionOutcome, PromotionError> {
    let mut journal = Journal::open(&layout.store_path())?;
    let identity = journal
        .last()
        .ok_or(PromotionError::EmptyJournal)?
        .identity
        .clone();

    if contains(&journal, Boundary::PromotionCommitted) {
        layout.verify_convergence(Convergence::Candidate, &identity)?;
        return Ok(PromotionOutcome {
            convergence: Convergence::Candidate,
            terminal_boundary: Boundary::PromotionCommitted,
            health: None,
        });
    }
    if contains(&journal, Boundary::SafeStopCompleted) {
        layout.verify_convergence(Convergence::SafeStopped, &identity)?;
        return Ok(PromotionOutcome {
            convergence: Convergence::SafeStopped,
            terminal_boundary: Boundary::SafeStopCompleted,
            health: None,
        });
    }
    if contains(&journal, Boundary::RollbackCompleted) {
        layout.verify_convergence(Convergence::Prior, &identity)?;
        return Ok(PromotionOutcome {
            convergence: Convergence::Prior,
            terminal_boundary: Boundary::RollbackCompleted,
            health: None,
        });
    }

    converge_prior(layout, &identity, &mut journal, crash_after, None)
}

fn converge_prior(
    layout: &Layout,
    identity: &PromotionIdentity,
    journal: &mut Journal,
    crash_after: CrashAfter,
    health: Option<HealthEvidence>,
) -> Result<PromotionOutcome, PromotionError> {
    append_once(journal, identity, Boundary::HealthRejected, crash_after)?;
    append_once(journal, identity, Boundary::RollbackStarted, crash_after)?;

    if identity.fault == FaultProfile::RollbackFailure {
        append_once(journal, identity, Boundary::RollbackRejected, crash_after)?;
        return safe_stop(layout, identity, journal, crash_after, health);
    }

    layout.switch_active(&identity.prior)?;
    append_once(
        journal,
        identity,
        Boundary::PriorPointerRestored,
        crash_after,
    )?;
    layout.mark_prior_started()?;
    append_once(journal, identity, Boundary::PriorStarted, crash_after)?;
    layout.set_effects_clean()?;
    layout.set_input(RuntimeState::Live)?;
    layout.set_shell(RuntimeState::Live)?;
    append_once(
        journal,
        identity,
        Boundary::RollbackHealthAccepted,
        crash_after,
    )?;
    layout.cleanup_snapshot(identity)?;
    append_once(journal, identity, Boundary::RollbackCompleted, crash_after)?;
    layout.verify_convergence(Convergence::Prior, identity)?;
    Ok(PromotionOutcome {
        convergence: Convergence::Prior,
        terminal_boundary: Boundary::RollbackCompleted,
        health,
    })
}

fn safe_stop(
    layout: &Layout,
    identity: &PromotionIdentity,
    journal: &mut Journal,
    crash_after: CrashAfter,
    health: Option<HealthEvidence>,
) -> Result<PromotionOutcome, PromotionError> {
    append_once(journal, identity, Boundary::SafeStopStarted, crash_after)?;
    layout.switch_active(&identity.prior)?;
    layout.set_effects_clean()?;
    append_once(journal, identity, Boundary::EffectsRestored, crash_after)?;
    layout.set_input(RuntimeState::Stopped)?;
    layout.set_shell(RuntimeState::Stopped)?;
    layout.mark_processes_stopped()?;
    append_once(journal, identity, Boundary::ProcessesStopped, crash_after)?;
    layout.cleanup_snapshot(identity)?;
    append_once(journal, identity, Boundary::SafeStopCompleted, crash_after)?;
    layout.verify_convergence(Convergence::SafeStopped, identity)?;
    Ok(PromotionOutcome {
        convergence: Convergence::SafeStopped,
        terminal_boundary: Boundary::SafeStopCompleted,
        health,
    })
}

fn append(
    journal: &mut Journal,
    identity: &PromotionIdentity,
    boundary: Boundary,
    crash_after: CrashAfter,
) -> Result<(), PromotionError> {
    journal.append(identity, boundary)?;
    crash_after.reached(boundary)
}

fn append_once(
    journal: &mut Journal,
    identity: &PromotionIdentity,
    boundary: Boundary,
    crash_after: CrashAfter,
) -> Result<(), PromotionError> {
    if contains(journal, boundary) {
        Ok(())
    } else {
        append(journal, identity, boundary, crash_after)
    }
}

fn contains(journal: &Journal, boundary: Boundary) -> bool {
    journal
        .records()
        .iter()
        .any(|record| record.boundary == boundary)
}

#[derive(Debug, Error)]
pub enum PromotionError {
    #[error("journal operation failed")]
    Journal(#[from] JournalError),
    #[error("installation operation failed")]
    Installation(#[from] InstallError),
    #[error("candidate health operation failed")]
    Candidate(#[from] crate::candidate::CandidateError),
    #[error("recovery journal is empty")]
    EmptyJournal,
    #[error("injected process death after durable boundary {0}")]
    InjectedProcessDeath(Boundary),
}
