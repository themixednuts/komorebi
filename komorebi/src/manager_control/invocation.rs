use komorebi_command_store::InvocationCommitDecision;
use komorebi_command_store::InvocationInspection;
use komorebi_command_store::OutcomeDocument;
use komorebi_command_store::RecoveryPolicy;
use komorebi_command_store::TransitionDecision;
use komorebi_protocol::ActionInvocation;
use komorebi_protocol::AuthoritySummary;
use komorebi_protocol::CommandCodecError;
use komorebi_protocol::InvocationProgress;
use komorebi_protocol::InvocationRejection;
use komorebi_protocol::InvocationStatus;
use komorebi_protocol::InvocationStatusCodec;
use komorebi_protocol::InvocationSubmissionReply;
use komorebi_protocol::InvocationTerminal;
use komorebi_protocol::PrincipalId;
use thiserror::Error;

use crate::action::ActionAdmission;
use crate::action::ActionAuthority;
use crate::action::ActionPreparation;
use crate::action::ActionRejection;
use crate::action::InvocationContext;
use crate::action::InvocationOrigin;
use crate::adapters::action_catalog::action_grants;
use crate::adapters::action_catalog::project_unavailability;
use crate::adapters::action_catalog::snapshot as project_catalog;
use crate::adapters::action_invocation;
use crate::adapters::action_invocation::InvocationBindingError;
use crate::command_control::ManagerInvocationLedger;
use crate::window_manager::WindowManager;

use super::ManagerControlError;

pub(super) fn submit(
    manager: &mut WindowManager,
    ledger: &ManagerInvocationLedger,
    principal: PrincipalId,
    authority: AuthoritySummary,
    invocation: ActionInvocation,
) -> Result<InvocationSubmissionReply, ManagerControlError> {
    match ledger.inspect(principal, invocation.clone())? {
        InvocationInspection::Retained(record) => {
            return Ok(InvocationSubmissionReply::Retained(record.status()));
        }
        InvocationInspection::IdempotencyConflict => {
            return Ok(rejected(InvocationRejection::IdempotencyConflict));
        }
        InvocationInspection::InvocationExpired => {
            return Ok(rejected(InvocationRejection::InvocationExpired));
        }
        InvocationInspection::UnknownNamespace => {
            return Ok(rejected(InvocationRejection::UnknownNamespace));
        }
        InvocationInspection::Vacant => {}
    }

    manager.refresh_catalog_observation()?;
    let grants = action_grants(&authority);
    let catalog = project_catalog(
        manager.catalog.snapshot(),
        &ActionAuthority {
            grants: grants.clone(),
        },
    )?;
    let request = match action_invocation::bind(&catalog, &invocation) {
        Ok(request) => request,
        Err(InvocationBindingError::Rejected(source)) => return Ok(rejected(source)),
        Err(InvocationBindingError::Arguments(_)) => {
            return Ok(rejected(InvocationRejection::InvalidArguments));
        }
        Err(InvocationBindingError::CatalogContract(source)) => return Err(source.into()),
    };
    let prepared = match manager.catalog.prepare(
        &request,
        &InvocationContext {
            principal,
            origin: InvocationOrigin::Ipc,
            grants,
        },
        std::time::Instant::now(),
    ) {
        ActionPreparation::Prepared(prepared) => prepared,
        ActionPreparation::Rejected { source, .. } => {
            return Ok(rejected(project_action_rejection(source)));
        }
        ActionPreparation::Retained(_) => {
            return Ok(rejected(InvocationRejection::IdempotencyConflict));
        }
    };

    let state = prepared.committed_state();
    let committed = match ledger.commit_invocation(
        principal,
        invocation,
        state,
        RecoveryPolicy::NeverReplay,
    )? {
        InvocationCommitDecision::Committed(committed) => committed,
        InvocationCommitDecision::Retained(record) => {
            return Ok(InvocationSubmissionReply::Retained(record.status()));
        }
        InvocationCommitDecision::IdempotencyConflict => {
            return Ok(rejected(InvocationRejection::IdempotencyConflict));
        }
        InvocationCommitDecision::InvocationExpired => {
            return Ok(rejected(InvocationRejection::InvocationExpired));
        }
        InvocationCommitDecision::InvocationNotLeased => {
            return Ok(rejected(InvocationRejection::InvocationNotLeased));
        }
        InvocationCommitDecision::UnknownNamespace => {
            return Ok(rejected(InvocationRejection::UnknownNamespace));
        }
        InvocationCommitDecision::CapacityFull => {
            return Ok(rejected(InvocationRejection::CapacityFull));
        }
    };

    let admission = manager
        .catalog
        .commit_prepared(prepared)
        .map_err(ManagerControlError::PostCommitManager)?;
    let ActionAdmission::Committed {
        state: committed_state,
        logical_result,
        effects,
        ..
    } = admission
    else {
        return Err(ManagerControlError::CommittedAdmissionRejected);
    };
    if committed_state != state {
        return Err(ManagerControlError::CommittedStateMismatch {
            durable: state,
            manager: committed_state,
        });
    }
    let dispatch_transition = ledger
        .mark_effect_dispatched(committed.invocation_id())
        .map_err(|source| ManagerControlError::PostCommitLedger {
            stage: "record effect dispatch",
            source,
        })?;
    if dispatch_transition != TransitionDecision::Applied {
        return Err(ManagerControlError::DispatchTransition);
    }

    let kind = manager.dispatch_committed_catalog_action(
        committed.invocation_id(),
        logical_result,
        &effects,
    );
    let status = InvocationStatus::new(
        committed.invocation_id(),
        committed.digest(),
        InvocationProgress::Terminal(InvocationTerminal::Settled { state, kind }),
    );
    let outcome = outcome_document(status).map_err(ManagerControlError::PostCommitDocument)?;
    let terminal_transition = ledger
        .record_terminal(committed.invocation_id(), kind, outcome)
        .map_err(|source| ManagerControlError::PostCommitLedger {
            stage: "record terminal outcome",
            source,
        })?;
    if terminal_transition != TransitionDecision::Applied {
        return Err(ManagerControlError::TerminalTransition);
    }
    Ok(InvocationSubmissionReply::Accepted(status))
}

fn outcome_document(status: InvocationStatus) -> Result<OutcomeDocument, InvocationDocumentError> {
    Ok(OutcomeDocument::new(
        std::num::NonZeroU16::MIN,
        InvocationStatusCodec::encode(status)?,
    )?)
}

#[derive(Debug, Error)]
pub enum InvocationDocumentError {
    #[error("canonical invocation status encoding failed: {0}")]
    Codec(#[from] CommandCodecError),
    #[error("versioned invocation status document failed: {0}")]
    Document(#[from] komorebi_command_store::DocumentError),
}

const fn rejected(source: InvocationRejection) -> InvocationSubmissionReply {
    InvocationSubmissionReply::Rejected(source)
}

fn project_action_rejection(source: ActionRejection) -> InvocationRejection {
    match source {
        ActionRejection::StaleState { actual, .. } => {
            InvocationRejection::StaleState { current: actual }
        }
        ActionRejection::RevisionExhausted => InvocationRejection::CapacityFull,
        ActionRejection::Unavailable(reason) => match project_unavailability(&reason) {
            komorebi_protocol::ActionUnavailability::Unauthorized => {
                InvocationRejection::Unauthorized
            }
            reason => InvocationRejection::Unavailable(reason),
        },
        ActionRejection::Confirmation(_) => InvocationRejection::ConfirmationUnavailable,
    }
}
