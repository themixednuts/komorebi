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
mod tests {
    use super::*;
    use crate::action::Pixels;
    use crate::action::id::WindowId;
    use crate::core::Axis;
    use crate::core::DefaultLayout;
    use crate::core::OperationDirection;
    use crate::core::ResizeStep;
    use crate::core::Sizing;
    use crate::core::TransparencyAlpha;

    fn invocation(sequence: u64) -> InvocationId {
        InvocationId::new(
            InvocationNamespaceId::new([9; 16]).expect("test namespace is nonzero"),
            InvocationSequence::try_from(sequence).expect("test sequence is nonzero"),
        )
    }

    fn principal(byte: u8) -> PrincipalId {
        PrincipalId::new([byte; 32]).expect("test principal is nonzero")
    }

    fn stamp_in(epoch: u8, revision: u64) -> StateStamp {
        StateStamp::new(
            komorebi_protocol::ManagerEpoch::new([epoch; 16]).expect("test epoch is non-nil"),
            komorebi_protocol::Revision::try_from(revision).expect("test revision is nonzero"),
        )
    }

    fn stamp(revision: u64) -> StateStamp {
        stamp_in(1, revision)
    }

    fn live_state() -> CatalogState {
        CatalogState::new(ActionSnapshot {
            state: stamp(10),
            paused: false,
            focused_window: Some(WindowId::new(1)),
            directional_targets: [OperationDirection::Left].into(),
            current_layout: DefaultLayout::BSP,
            configuration: crate::action::ConfigurationSnapshot::default(),
            focused_window_floating: false,
            named_workspaces: Vec::new(),
            bindings: Vec::new(),
        })
    }

    fn context() -> InvocationContext {
        InvocationContext {
            principal: principal(1),
            origin: InvocationOrigin::Cli,
            grants: ActionGrants::all(),
        }
    }

    fn focus_left(id: InvocationId, state: StateStamp) -> InvokeAction {
        InvokeAction {
            invocation_id: id,
            expected_state: state,
            action: BuiltinAction::FocusWindow {
                direction: OperationDirection::Left,
            },
            confirmation: None,
        }
    }

    #[test]
    fn stale_state_is_rejected() {
        let mut state = live_state();
        let admission = state.admit(
            focus_left(invocation(1), stamp_in(2, 10)),
            &context(),
            Instant::now(),
        );
        assert!(matches!(
            admission,
            ActionAdmission::Rejected(ActionRejection::StaleState { .. })
        ));
    }

    #[test]
    fn exhausted_revision_rejects_without_mutating_logical_state() {
        let mut state = live_state();
        state.snapshot.state = stamp(u64::MAX);
        let invocation_id = invocation(2);

        let admission = state.admit(
            focus_left(invocation_id, stamp(u64::MAX)),
            &context(),
            Instant::now(),
        );

        assert_eq!(
            admission,
            ActionAdmission::Rejected(ActionRejection::RevisionExhausted)
        );
        assert_eq!(state.snapshot().state, stamp(u64::MAX));
        assert_eq!(state.status(invocation_id), None);
    }

    #[test]
    fn missing_neighbor_is_rejected_with_a_typed_reason() {
        let mut state = live_state();
        let admission = state.admit(
            InvokeAction {
                invocation_id: invocation(3),
                expected_state: stamp(10),
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
        let id = invocation(4);
        let first = state.admit(focus_left(id, stamp(10)), &context(), Instant::now());
        let second = state.admit(focus_left(id, stamp(10)), &context(), Instant::now());
        match (first, second) {
            (
                ActionAdmission::Committed {
                    state: first_state, ..
                },
                ActionAdmission::Committed {
                    state: second_state,
                    ..
                },
            ) => {
                assert_eq!(first_state, second_state);
                assert_eq!(first_state, stamp(11));
            }
            _ => panic!("both admissions should be the same commit"),
        }
    }

    #[test]
    fn preparation_is_read_only_until_explicit_commit() {
        let mut state = live_state();
        let id = invocation(6);
        let before = state.snapshot().clone();
        let request = focus_left(id, before.state);

        let ActionPreparation::Prepared(prepared) =
            state.prepare(&request, &context(), Instant::now())
        else {
            panic!("live action should prepare");
        };

        assert_eq!(state.snapshot(), &before);
        assert_eq!(state.status(id), None);
        assert_eq!(prepared.previous_state(), stamp(10));
        assert_eq!(prepared.committed_state(), stamp(11));

        let admission = state
            .commit_prepared(prepared)
            .expect("prepared transition should commit");
        assert!(matches!(
            admission,
            ActionAdmission::Committed { state, .. } if state == stamp(11)
        ));
        assert_eq!(state.snapshot().state, stamp(11));
    }

    #[test]
    fn configured_resize_step_resolves_to_one_exact_effect() {
        let mut state = live_state();
        state.snapshot.configuration.resize_step =
            ResizeStep::new(73).expect("test step is positive");
        let request = InvokeAction {
            invocation_id: invocation(12),
            expected_state: stamp(10),
            action: BuiltinAction::ResizeWindowByStep {
                axis: Axis::Horizontal,
                sizing: Sizing::Decrease,
            },
            confirmation: None,
        };

        let ActionPreparation::Prepared(prepared) =
            state.prepare(&request, &context(), Instant::now())
        else {
            panic!("live resize action should prepare");
        };
        let expected = Pixels::new(-73).expect("resolved delta is nonzero");

        assert_eq!(
            prepared.logical_result,
            ActionResult::Resized {
                axis: Axis::Horizontal,
                delta: expected,
            }
        );
        assert_eq!(
            prepared.effects,
            vec![PlannedEffect {
                id: EffectId::new(0),
                effect: NativeEffect::Resize {
                    axis: Axis::Horizontal,
                    delta: expected,
                },
            }]
        );
    }

    #[test]
    fn setting_resize_step_updates_the_snapshot_and_plans_one_exact_effect() {
        let mut state = live_state();
        let step = ResizeStep::new(91).expect("test resize step is positive");
        let request = InvokeAction {
            invocation_id: invocation(13),
            expected_state: stamp(10),
            action: BuiltinAction::SetResizeStep { step },
            confirmation: None,
        };

        let ActionPreparation::Prepared(prepared) =
            state.prepare(&request, &context(), Instant::now())
        else {
            panic!("set-resize-step should prepare");
        };

        assert_eq!(
            prepared.logical_result,
            ActionResult::ResizeStepSet { step }
        );
        assert_eq!(
            prepared.effects,
            vec![PlannedEffect {
                id: EffectId::new(0),
                effect: NativeEffect::SetResizeStep { step },
            }]
        );

        state
            .commit_prepared(prepared)
            .expect("prepared resize-step transition should commit");
        assert_eq!(state.snapshot().configuration.resize_step, step);
    }

    #[test]
    fn transparency_toggle_and_alpha_resolve_to_exact_configuration_effects() {
        let mut state = live_state();
        let toggle = InvokeAction {
            invocation_id: invocation(14),
            expected_state: stamp(10),
            action: BuiltinAction::ToggleTransparency,
            confirmation: None,
        };
        let ActionPreparation::Prepared(prepared) =
            state.prepare(&toggle, &context(), Instant::now())
        else {
            panic!("toggle-transparency should prepare");
        };
        assert_eq!(
            prepared.logical_result,
            ActionResult::TransparencyToggled { enabled: true }
        );
        assert_eq!(
            prepared.effects,
            vec![PlannedEffect {
                id: EffectId::new(0),
                effect: NativeEffect::SetTransparencyEnabled { enabled: true },
            }]
        );
        state
            .commit_prepared(prepared)
            .expect("prepared transparency toggle should commit");
        assert!(state.snapshot().configuration.transparency.enabled);

        let alpha = TransparencyAlpha::new(177);
        let set_alpha = InvokeAction {
            invocation_id: invocation(15),
            expected_state: stamp(11),
            action: BuiltinAction::SetTransparencyAlpha { alpha },
            confirmation: None,
        };
        let ActionPreparation::Prepared(prepared) =
            state.prepare(&set_alpha, &context(), Instant::now())
        else {
            panic!("set-transparency-alpha should prepare");
        };
        assert_eq!(
            prepared.logical_result,
            ActionResult::TransparencyAlphaSet { alpha }
        );
        assert_eq!(
            prepared.effects,
            vec![PlannedEffect {
                id: EffectId::new(0),
                effect: NativeEffect::SetTransparencyAlpha { alpha },
            }]
        );
        state
            .commit_prepared(prepared)
            .expect("prepared transparency alpha should commit");
        assert_eq!(state.snapshot().configuration.transparency.alpha, alpha);
    }

    #[test]
    fn prepared_transition_rejects_publication_after_state_advances() {
        let mut state = live_state();
        let first = match state.prepare(
            &focus_left(invocation(7), stamp(10)),
            &context(),
            Instant::now(),
        ) {
            ActionPreparation::Prepared(prepared) => prepared,
            _ => panic!("first action should prepare"),
        };
        let second_id = invocation(8);
        let second = match state.prepare(
            &focus_left(second_id, stamp(10)),
            &context(),
            Instant::now(),
        ) {
            ActionPreparation::Prepared(prepared) => prepared,
            _ => panic!("second action should prepare"),
        };

        state
            .commit_prepared(first)
            .expect("first transition should commit");
        assert_eq!(
            state.commit_prepared(second),
            Err(PreparedCommitError::StateChanged {
                expected: stamp(10),
                actual: stamp(11),
            })
        );
        assert_eq!(state.status(second_id), None);
    }

    #[test]
    fn identical_observation_preserves_the_exact_revision() {
        let mut state = live_state();
        let observation = state.snapshot().clone();

        let change = state
            .reconcile_observation(observation)
            .expect("unchanged observation should reconcile");

        assert_eq!(change, ObservationChange::Unchanged { state: stamp(10) });
        assert_eq!(state.snapshot().state, stamp(10));
    }

    #[test]
    fn manager_local_invocation_ids_are_namespaced_and_contiguous() {
        let mut state = live_state();

        let first = state
            .issue_local_invocation_id()
            .expect("first local identity should issue");
        let second = state
            .issue_local_invocation_id()
            .expect("second local identity should issue");

        assert_eq!(first.namespace(), second.namespace());
        assert_eq!(first.sequence().get(), 1);
        assert_eq!(second.sequence().get(), 2);
        assert_eq!(
            first.namespace().into_bytes(),
            stamp(10).epoch().into_bytes()
        );
    }

    #[test]
    fn semantic_observation_change_advances_exactly_once() {
        let mut state = live_state();
        let mut observation = state.snapshot().clone();
        observation.paused = true;
        observation.state = stamp_in(2, 99);

        let change = state
            .reconcile_observation(observation.clone())
            .expect("changed observation should reconcile");

        assert_eq!(
            change,
            ObservationChange::Advanced {
                previous: stamp(10),
                current: stamp(11),
            }
        );
        assert!(state.snapshot().paused);
        assert_eq!(state.snapshot().state, stamp(11));
        assert_eq!(
            state
                .reconcile_observation(observation)
                .expect("repeated observation should reconcile"),
            ObservationChange::Unchanged { state: stamp(11) }
        );
    }

    #[test]
    fn exhausted_observation_revision_does_not_mutate_catalog() {
        let mut state = live_state();
        state.snapshot.state = stamp(u64::MAX);
        let before = state.snapshot().clone();
        let mut observation = before.clone();
        observation.paused = true;

        let error = state
            .reconcile_observation(observation)
            .expect_err("exhausted revision must reject a semantic change");

        assert_eq!(
            error,
            komorebi_protocol::ActionContractError::RevisionExhausted
        );
        assert_eq!(state.snapshot(), &before);
    }

    #[test]
    fn injected_outcomes_produce_settled_degraded_superseded_and_reconcile() {
        let mut state = live_state();
        let id = invocation(5);
        let admission = state.admit(focus_left(id, stamp(10)), &context(), Instant::now());
        let ActionAdmission::Committed {
            state: committed_state,
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
                state: committed_state,
                result: logical_result
            })
        );
        state.degrade(
            id,
            vec![NativeEffectFailure {
                effect_id: EffectId::new(0),
                message: "injected failure".to_string(),
            }],
        );
        // settle already moved it; degrade only from Committed. seed a committed status.
        state.statuses.insert(
            id,
            InvocationStatus::Committed {
                state: committed_state,
            },
        );
        let failures = vec![
            NativeEffectFailure {
                effect_id: EffectId::new(0),
                message: "first injected failure".to_string(),
            },
            NativeEffectFailure {
                effect_id: EffectId::new(1),
                message: "second injected failure".to_string(),
            },
        ];
        state.degrade(id, failures.clone());
        assert_eq!(
            state.status(id),
            Some(&InvocationStatus::Degraded {
                state: committed_state,
                failures
            })
        );
        state.supersede(id, stamp(12));
        assert_eq!(
            state.status(id),
            Some(&InvocationStatus::Superseded {
                by_state: stamp(12)
            })
        );
        state.reconcile_after_restart(id, committed_state);
        assert_eq!(
            state.status(id),
            Some(&InvocationStatus::ReconcilingAfterRestart {
                state: committed_state
            })
        );
    }
}
