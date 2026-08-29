use std::collections::HashMap;
use std::time::Instant;

use super::builtin::BuiltinAction;
use super::confirmation::ConfirmationError;
use super::confirmation::ConfirmationLedger;
use super::definition::UndoPolicy;
use super::id::InvocationId;
use super::id::PrincipalId;
use super::id::Revision;
use super::offer::ActionAvailability;
use super::offer::ActionGrants;
use super::offer::ActionSnapshot;
use super::offer::Unavailability;
pub use super::outcome::ActionResult;
pub use super::outcome::NativeEffect;
use super::transition::apply_logical;
use super::transition::bind_named_targets;
use super::transition::directional_gap;
use super::transition::effects;
use super::transition::logical_result;
use super::undo::UndoLedger;
use super::undo::UndoRecord;

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
    pub expected_revision: Revision,
    pub action: BuiltinAction,
    pub confirmation: Option<super::id::ConfirmationToken>,
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ActionRejection {
    #[error("expected revision {expected:?} but catalog is at {actual:?}")]
    StaleRevision {
        expected: Revision,
        actual: Revision,
    },
    #[error("action is unavailable: {0:?}")]
    Unavailable(Unavailability),
    #[error(transparent)]
    Confirmation(ConfirmationError),
}

#[derive(Clone, Debug, PartialEq)]
pub enum ActionAdmission {
    Rejected(ActionRejection),
    Committed {
        revision: Revision,
        logical_result: ActionResult,
        undo: Option<UndoRecord>,
        effects: Vec<NativeEffect>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InvocationStatus {
    Committed {
        revision: Revision,
    },
    Converging {
        revision: Revision,
        pending: usize,
    },
    Settled {
        revision: Revision,
        result: ActionResult,
    },
    Degraded {
        revision: Revision,
        failures: usize,
    },
    Superseded {
        by_revision: Revision,
    },
    Cancelled,
    ReconcilingAfterRestart {
        revision: Revision,
    },
}

#[derive(Clone, Debug)]
pub struct CatalogState {
    snapshot: ActionSnapshot,
    invocations: HashMap<InvocationId, ActionAdmission>,
    statuses: HashMap<InvocationId, InvocationStatus>,
    confirmations: ConfirmationLedger,
    undos: UndoLedger,
}

impl CatalogState {
    #[must_use]
    pub fn new(snapshot: ActionSnapshot) -> Self {
        Self {
            snapshot,
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

    pub fn replace_observation(&mut self, mut snapshot: ActionSnapshot) {
        snapshot.revision = self.snapshot.revision;
        self.snapshot = snapshot;
    }

    #[must_use]
    pub fn status(&self, id: InvocationId) -> Option<&InvocationStatus> {
        self.statuses.get(&id)
    }

    pub fn confirmations_mut(&mut self) -> &mut ConfirmationLedger {
        &mut self.confirmations
    }

    pub fn admit(
        &mut self,
        request: InvokeAction,
        context: &InvocationContext,
        now: Instant,
    ) -> ActionAdmission {
        if let Some(previous) = self.invocations.get(&request.invocation_id) {
            return previous.clone();
        }

        if request.expected_revision != self.snapshot.revision {
            return store(
                &mut self.invocations,
                request.invocation_id,
                ActionAdmission::Rejected(ActionRejection::StaleRevision {
                    expected: request.expected_revision,
                    actual: self.snapshot.revision,
                }),
            );
        }

        if !context.grants.contains(request.action.kind()) {
            return store(
                &mut self.invocations,
                request.invocation_id,
                ActionAdmission::Rejected(ActionRejection::Unavailable(
                    Unavailability::Unauthorized,
                )),
            );
        }

        if let Some(token) = request.confirmation
            && let Err(error) = self.confirmations.consume(
                token,
                context.principal,
                &request.action,
                request.expected_revision,
                now,
            )
        {
            return store(
                &mut self.invocations,
                request.invocation_id,
                ActionAdmission::Rejected(ActionRejection::Confirmation(error)),
            );
        }

        let availability = super::offer::offers(
            &self.snapshot,
            &super::offer::ActionAuthority {
                principal: context.principal,
                grants: context.grants.clone(),
            },
        )
        .into_iter()
        .find(|offer| offer.definition.kind == request.action.kind())
        .map(|offer| offer.availability);

        match availability {
            Some(ActionAvailability::Unavailable(reason)) => store(
                &mut self.invocations,
                request.invocation_id,
                ActionAdmission::Rejected(ActionRejection::Unavailable(reason)),
            ),
            Some(ActionAvailability::Available) => {
                let action = match bind_named_targets(&self.snapshot, &request.action) {
                    Ok(action) => action,
                    Err(reason) => {
                        return store(
                            &mut self.invocations,
                            request.invocation_id,
                            ActionAdmission::Rejected(ActionRejection::Unavailable(reason)),
                        );
                    }
                };
                if let Some(reason) = directional_gap(&self.snapshot, &action) {
                    return store(
                        &mut self.invocations,
                        request.invocation_id,
                        ActionAdmission::Rejected(ActionRejection::Unavailable(reason)),
                    );
                }
                let revision = self.snapshot.revision.next();
                self.snapshot.revision = revision;
                apply_logical(&mut self.snapshot, &action);
                let undo = request
                    .action
                    .kind()
                    .definition()
                    .undo
                    .ne(&UndoPolicy::None)
                    .then(|| {
                        self.undos
                            .issue(request.action.kind().definition().undo)
                            .ok()
                    })
                    .flatten();
                let committed = ActionAdmission::Committed {
                    revision,
                    logical_result: logical_result(&action, &self.snapshot),
                    undo,
                    effects: effects(&action, &self.snapshot),
                };
                self.statuses.insert(
                    request.invocation_id,
                    InvocationStatus::Committed { revision },
                );
                store(&mut self.invocations, request.invocation_id, committed)
            }
            None => store(
                &mut self.invocations,
                request.invocation_id,
                ActionAdmission::Rejected(ActionRejection::Unavailable(
                    Unavailability::Unauthorized,
                )),
            ),
        }
    }

    pub fn settle(&mut self, id: InvocationId, result: ActionResult) {
        if let Some(InvocationStatus::Committed { revision }) = self.statuses.get(&id).cloned() {
            self.statuses
                .insert(id, InvocationStatus::Settled { revision, result });
        }
    }

    pub fn degrade(&mut self, id: InvocationId, failures: usize) {
        if let Some(InvocationStatus::Committed { revision }) = self.statuses.get(&id).cloned() {
            self.statuses
                .insert(id, InvocationStatus::Degraded { revision, failures });
        }
    }

    pub fn supersede(&mut self, id: InvocationId, by_revision: Revision) {
        self.statuses
            .insert(id, InvocationStatus::Superseded { by_revision });
    }

    pub fn reconcile_after_restart(&mut self, id: InvocationId, revision: Revision) {
        self.statuses
            .insert(id, InvocationStatus::ReconcilingAfterRestart { revision });
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::id::WindowId;
    use crate::core::DefaultLayout;
    use crate::core::OperationDirection;

    fn live_state() -> CatalogState {
        CatalogState::new(ActionSnapshot {
            revision: Revision::new(10),
            paused: false,
            focused_window: Some(WindowId::new(1)),
            neighbor_left: true,
            neighbor_right: false,
            neighbor_up: false,
            neighbor_down: false,
            current_layout: DefaultLayout::BSP,
            focused_window_floating: false,
            named_workspaces: Vec::new(),
            bindings: Vec::new(),
        })
    }

    fn context() -> InvocationContext {
        InvocationContext {
            principal: PrincipalId::new(1),
            origin: InvocationOrigin::Cli,
            grants: ActionGrants::all(),
        }
    }

    fn focus_left(id: InvocationId, revision: Revision) -> InvokeAction {
        InvokeAction {
            invocation_id: id,
            expected_revision: revision,
            action: BuiltinAction::FocusWindow {
                direction: OperationDirection::Left,
            },
            confirmation: None,
        }
    }

    #[test]
    fn stale_revision_is_rejected() {
        let mut state = live_state();
        let admission = state.admit(
            focus_left(InvocationId::new(), Revision::new(1)),
            &context(),
            Instant::now(),
        );
        assert!(matches!(
            admission,
            ActionAdmission::Rejected(ActionRejection::StaleRevision { .. })
        ));
    }

    #[test]
    fn missing_neighbor_is_rejected_with_a_typed_reason() {
        let mut state = live_state();
        let admission = state.admit(
            InvokeAction {
                invocation_id: InvocationId::new(),
                expected_revision: Revision::new(10),
                action: BuiltinAction::FocusWindow {
                    direction: OperationDirection::Right,
                },
                confirmation: None,
            },
            &context(),
            Instant::now(),
        );
        assert_eq!(
            admission,
            ActionAdmission::Rejected(ActionRejection::Unavailable(
                Unavailability::NoWindowInDirection
            ))
        );
    }

    #[test]
    fn one_invocation_id_commits_once() {
        let mut state = live_state();
        let id = InvocationId::new();
        let first = state.admit(
            focus_left(id, Revision::new(10)),
            &context(),
            Instant::now(),
        );
        let second = state.admit(
            focus_left(id, Revision::new(10)),
            &context(),
            Instant::now(),
        );
        match (first, second) {
            (
                ActionAdmission::Committed {
                    revision: first_revision,
                    ..
                },
                ActionAdmission::Committed {
                    revision: second_revision,
                    ..
                },
            ) => {
                assert_eq!(first_revision, second_revision);
                assert_eq!(first_revision, Revision::new(11));
            }
            _ => panic!("both admissions should be the same commit"),
        }
    }

    #[test]
    fn injected_outcomes_produce_settled_degraded_superseded_and_reconcile() {
        let mut state = live_state();
        let id = InvocationId::new();
        let admission = state.admit(
            focus_left(id, Revision::new(10)),
            &context(),
            Instant::now(),
        );
        let ActionAdmission::Committed {
            revision,
            logical_result,
            ..
        } = admission
        else {
            panic!("expected commit");
        };
        state.settle(id, logical_result);
        assert_eq!(
            state.status(id),
            Some(&InvocationStatus::Settled {
                revision,
                result: logical_result
            })
        );
        state.degrade(id, 1);
        // settle already moved it; degrade only from Committed. seed a committed status.
        state
            .statuses
            .insert(id, InvocationStatus::Committed { revision });
        state.degrade(id, 2);
        assert_eq!(
            state.status(id),
            Some(&InvocationStatus::Degraded {
                revision,
                failures: 2
            })
        );
        state.supersede(id, Revision::new(12));
        assert_eq!(
            state.status(id),
            Some(&InvocationStatus::Superseded {
                by_revision: Revision::new(12)
            })
        );
        state.reconcile_after_restart(id, revision);
        assert_eq!(
            state.status(id),
            Some(&InvocationStatus::ReconcilingAfterRestart { revision })
        );
    }
}
