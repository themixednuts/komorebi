use std::collections::HashMap;
use std::time::Instant;

use super::builtin::BuiltinAction;
use super::confirmation::ConfirmationError;
use super::confirmation::ConfirmationLedger;
use super::confirmation::ValidatedConfirmation;
use super::definition::UndoPolicy;
use super::id::InvocationId;
use super::id::PrincipalId;
use super::offer::ActionAvailability;
use super::offer::ActionGrants;
use super::offer::ActionSnapshot;
use super::offer::Unavailability;
pub use super::outcome::ActionResult;
pub use super::outcome::EffectId;
pub use super::outcome::NativeEffect;
pub use super::outcome::NativeEffectFailure;
pub use super::outcome::PlannedEffect;
use super::transition::apply_logical;
use super::transition::directional_gap;
use super::transition::effects;
use super::transition::logical_result;
use super::transition::resolve_contextual_inputs;
use super::undo::UndoLedger;
use super::undo::UndoRecord;
use komorebi_protocol::InvocationIdentityError;
use komorebi_protocol::InvocationNamespaceId;
use komorebi_protocol::InvocationSequence;
use komorebi_protocol::StateStamp;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObservationChange {
    Unchanged {
        state: StateStamp,
    },
    Advanced {
        previous: StateStamp,
        current: StateStamp,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InvocationOrigin {
    Palette,
    Input,
    Cli,
    Ipc,
    Lua,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InvocationContext {
    pub principal: PrincipalId,
    pub origin: InvocationOrigin,
    pub grants: ActionGrants,
}

#[derive(Clone, Debug)]
pub struct InvokeAction {
    pub invocation_id: InvocationId,
    pub expected_state: StateStamp,
    pub action: BuiltinAction,
    pub confirmation: Option<super::id::ConfirmationToken>,
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ActionRejection {
    #[error("expected state {expected:?} but manager is at {actual:?}")]
    StaleState {
        expected: StateStamp,
        actual: StateStamp,
    },
    #[error("manager state revision is exhausted")]
    RevisionExhausted,
    #[error("action is unavailable: {0:?}")]
    Unavailable(Unavailability),
    #[error(transparent)]
    Confirmation(ConfirmationError),
}

#[derive(Clone, Debug, PartialEq)]
pub enum ActionAdmission {
    Rejected(ActionRejection),
    Committed {
        state: StateStamp,
        logical_result: ActionResult,
        undo: Option<UndoRecord>,
        effects: Vec<PlannedEffect>,
    },
}

#[derive(Debug)]
pub enum ActionPreparation {
    Retained(ActionAdmission),
    Rejected {
        invocation_id: InvocationId,
        source: ActionRejection,
    },
    Prepared(PreparedAction),
}

#[derive(Debug)]
pub struct PreparedAction {
    invocation_id: InvocationId,
    previous_state: StateStamp,
    snapshot: ActionSnapshot,
    logical_result: ActionResult,
    undo: Option<UndoRecord>,
    effects: Vec<PlannedEffect>,
    confirmation: Option<ValidatedConfirmation>,
}

impl PreparedAction {
    #[must_use]
    pub const fn invocation_id(&self) -> InvocationId {
        self.invocation_id
    }

    #[must_use]
    pub const fn previous_state(&self) -> StateStamp {
        self.previous_state
    }

    #[must_use]
    pub const fn committed_state(&self) -> StateStamp {
        self.snapshot.state
    }

    #[must_use]
    pub fn logical_result(&self) -> &ActionResult {
        &self.logical_result
    }

    #[must_use]
    pub fn effects(&self) -> &[PlannedEffect] {
        &self.effects
    }
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum PreparedCommitError {
    #[error("prepared action expected state {expected:?} but manager is at {actual:?}")]
    StateChanged {
        expected: StateStamp,
        actual: StateStamp,
    },
    #[error("prepared invocation was already recorded")]
    InvocationAlreadyRecorded,
    #[error(transparent)]
    Confirmation(#[from] ConfirmationError),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InvocationStatus {
    Committed {
        state: StateStamp,
    },
    Converging {
        state: StateStamp,
        pending: usize,
    },
    Settled {
        state: StateStamp,
        result: ActionResult,
    },
    Degraded {
        state: StateStamp,
        failures: Vec<NativeEffectFailure>,
    },
    Superseded {
        by_state: StateStamp,
    },
    Cancelled,
    ReconcilingAfterRestart {
        state: StateStamp,
    },
}

#[derive(Clone, Debug)]
pub struct CatalogState {
    snapshot: ActionSnapshot,
    local_namespace: InvocationNamespaceId,
    next_local_sequence: InvocationSequence,
    invocations: HashMap<InvocationId, ActionAdmission>,
    statuses: HashMap<InvocationId, InvocationStatus>,
    confirmations: ConfirmationLedger,
    undos: UndoLedger,
}

impl CatalogState {
    #[must_use]
    pub fn new(snapshot: ActionSnapshot) -> Self {
        let local_namespace = match InvocationNamespaceId::new(snapshot.state.epoch().into_bytes())
        {
            Ok(namespace) => namespace,
            Err(_) => unreachable!("a validated manager epoch is a nonzero invocation namespace"),
        };
        Self {
            snapshot,
            local_namespace,
            next_local_sequence: InvocationSequence::new(std::num::NonZeroU64::MIN),
            invocations: HashMap::new(),
            statuses: HashMap::new(),
            confirmations: ConfirmationLedger::new(),
            undos: UndoLedger::new(),
        }
    }

    #[must_use]
    pub fn snapshot(&self) -> &ActionSnapshot {
        &self.snapshot
    }

    /// Reconciles a fresh manager observation with the catalog's logical state.
    ///
    /// # Errors
    ///
    /// Returns [`komorebi_protocol::ActionContractError::RevisionExhausted`]
    /// without changing the catalog if a semantic change cannot be assigned a
    /// new revision.
    pub fn reconcile_observation(
        &mut self,
        mut snapshot: ActionSnapshot,
    ) -> Result<ObservationChange, komorebi_protocol::ActionContractError> {
        let previous = self.snapshot.state;
        snapshot.state = previous;
        if snapshot == self.snapshot {
            return Ok(ObservationChange::Unchanged { state: previous });
        }

        let current = previous.next()?;
        snapshot.state = current;
        self.snapshot = snapshot;
        Ok(ObservationChange::Advanced { previous, current })
    }

    #[must_use]
    pub fn status(&self, id: InvocationId) -> Option<&InvocationStatus> {
        self.statuses.get(&id)
    }

    pub fn confirmations_mut(&mut self) -> &mut ConfirmationLedger {
        &mut self.confirmations
    }

    /// Issues an identity for an invocation originating inside the manager.
    ///
    /// # Errors
    ///
    /// Returns [`InvocationIdentityError::SequenceExhausted`] without issuing
    /// an identity when the process-lifetime local sequence is exhausted.
    pub fn issue_local_invocation_id(&mut self) -> Result<InvocationId, InvocationIdentityError> {
        let sequence = self.next_local_sequence;
        self.next_local_sequence = sequence.next()?;
        Ok(InvocationId::new(self.local_namespace, sequence))
    }

    #[must_use]
    pub fn prepare(
        &self,
        request: &InvokeAction,
        context: &InvocationContext,
        now: Instant,
    ) -> ActionPreparation {
        if let Some(previous) = self.invocations.get(&request.invocation_id) {
            return ActionPreparation::Retained(previous.clone());
        }

        if request.expected_state != self.snapshot.state {
            return rejected(
                request.invocation_id,
                ActionRejection::StaleState {
                    expected: request.expected_state,
                    actual: self.snapshot.state,
                },
            );
        }

        if !context.grants.contains(request.action.kind()) {
            return rejected(
                request.invocation_id,
                ActionRejection::Unavailable(Unavailability::Unauthorized),
            );
        }

        let confirmation = match request.confirmation {
            Some(token) => match self.confirmations.validate(
                token,
                context.principal,
                &request.action,
                request.expected_state,
                now,
            ) {
                Ok(validated) => Some(validated),
                Err(error) => {
                    return rejected(request.invocation_id, ActionRejection::Confirmation(error));
                }
            },
            None => None,
        };

        let availability = super::offer::offers(
            &self.snapshot,
            &super::offer::ActionAuthority {
                grants: context.grants.clone(),
            },
        )
        .into_iter()
        .find(|offer| offer.definition.kind == request.action.kind())
        .map(|offer| offer.availability);

        match availability {
            Some(ActionAvailability::Unavailable(reason)) => {
                rejected(request.invocation_id, ActionRejection::Unavailable(reason))
            }
            Some(ActionAvailability::Available) => {
                let action = match resolve_contextual_inputs(&self.snapshot, &request.action) {
                    Ok(action) => action,
                    Err(reason) => {
                        return rejected(
                            request.invocation_id,
                            ActionRejection::Unavailable(reason),
                        );
                    }
                };
                if let Some(reason) = directional_gap(&self.snapshot, &action) {
                    return rejected(request.invocation_id, ActionRejection::Unavailable(reason));
                }
                let Ok(state) = self.snapshot.state.next() else {
                    return rejected(request.invocation_id, ActionRejection::RevisionExhausted);
                };
                let mut snapshot = self.snapshot.clone();
                snapshot.state = state;
                apply_logical(&mut snapshot, &action);
                let undo = request
                    .action
                    .kind()
                    .definition()
                    .undo
                    .ne(&UndoPolicy::None)
                    .then(|| UndoLedger::prepare(request.action.kind().definition().undo).ok())
                    .flatten();
                ActionPreparation::Prepared(PreparedAction {
                    invocation_id: request.invocation_id,
                    previous_state: self.snapshot.state,
                    logical_result: logical_result(&action, &snapshot),
                    undo,
                    effects: effects(&action, &snapshot)
                        .into_iter()
                        .enumerate()
                        .map(|(ordinal, effect)| PlannedEffect {
                            id: EffectId::new(ordinal as u64),
                            effect,
                        })
                        .collect(),
                    snapshot,
                    confirmation,
                })
            }
            None => rejected(
                request.invocation_id,
                ActionRejection::Unavailable(Unavailability::Unauthorized),
            ),
        }
    }

    pub fn commit_prepared(
        &mut self,
        prepared: PreparedAction,
    ) -> Result<ActionAdmission, PreparedCommitError> {
        if self.invocations.contains_key(&prepared.invocation_id) {
            return Err(PreparedCommitError::InvocationAlreadyRecorded);
        }
        if self.snapshot.state != prepared.previous_state {
            return Err(PreparedCommitError::StateChanged {
                expected: prepared.previous_state,
                actual: self.snapshot.state,
            });
        }
        if let Some(confirmation) = prepared.confirmation {
            self.confirmations.consume_validated(confirmation)?;
        }
        if let Some(undo) = prepared.undo {
            self.undos.commit(undo);
        }

        let state = prepared.snapshot.state;
        self.snapshot = prepared.snapshot;
        let admission = ActionAdmission::Committed {
            state,
            logical_result: prepared.logical_result,
            undo: prepared.undo,
            effects: prepared.effects,
        };
        self.statuses.insert(
            prepared.invocation_id,
            InvocationStatus::Committed { state },
        );
        Ok(store(
            &mut self.invocations,
            prepared.invocation_id,
            admission,
        ))
    }

    pub fn admit(
        &mut self,
        request: InvokeAction,
        context: &InvocationContext,
        now: Instant,
    ) -> ActionAdmission {
        match self.prepare(&request, context, now) {
            ActionPreparation::Retained(previous) => previous,
            ActionPreparation::Rejected {
                invocation_id,
                source,
            } => store(
                &mut self.invocations,
                invocation_id,
                ActionAdmission::Rejected(source),
            ),
            ActionPreparation::Prepared(prepared) => {
                let invocation_id = prepared.invocation_id;
                match self.commit_prepared(prepared) {
                    Ok(admission) => admission,
                    Err(PreparedCommitError::StateChanged { expected, actual }) => store(
                        &mut self.invocations,
                        invocation_id,
                        ActionAdmission::Rejected(ActionRejection::StaleState { expected, actual }),
                    ),
                    Err(PreparedCommitError::Confirmation(error)) => store(
                        &mut self.invocations,
                        invocation_id,
                        ActionAdmission::Rejected(ActionRejection::Confirmation(error)),
                    ),
                    Err(PreparedCommitError::InvocationAlreadyRecorded) => {
                        match self.invocations.get(&invocation_id) {
                            Some(previous) => previous.clone(),
                            None => unreachable!("duplicate commit requires a retained admission"),
                        }
                    }
                }
            }
        }
    }

    pub fn settle(&mut self, id: InvocationId, result: ActionResult) {
        if let Some(InvocationStatus::Committed { state }) = self.statuses.get(&id).cloned() {
            self.statuses
                .insert(id, InvocationStatus::Settled { state, result });
        }
    }

    pub fn degrade(&mut self, id: InvocationId, failures: Vec<NativeEffectFailure>) {
        if let Some(InvocationStatus::Committed { state }) = self.statuses.get(&id).cloned() {
            self.statuses
                .insert(id, InvocationStatus::Degraded { state, failures });
        }
    }

    pub fn supersede(&mut self, id: InvocationId, by_state: StateStamp) {
        self.statuses
            .insert(id, InvocationStatus::Superseded { by_state });
    }

    pub fn reconcile_after_restart(&mut self, id: InvocationId, state: StateStamp) {
        self.statuses
            .insert(id, InvocationStatus::ReconcilingAfterRestart { state });
    }
}

fn store(
    invocations: &mut HashMap<InvocationId, ActionAdmission>,
    id: InvocationId,
    admission: ActionAdmission,
) -> ActionAdmission {
    invocations.insert(id, admission.clone());
    admission
}

fn rejected(invocation_id: InvocationId, source: ActionRejection) -> ActionPreparation {
    ActionPreparation::Rejected {
        invocation_id,
        source,
    }
}

#[cfg(test)]
#[path = "invoke_tests.rs"]
mod tests;
