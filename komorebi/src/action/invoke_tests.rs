use super::*;
use crate::action::Pixels;
use crate::action::id::WindowId;
use crate::core::Axis;
use crate::core::BorderImplementation;
use crate::core::BorderOffset;
use crate::core::BorderStyle;
use crate::core::BorderWidth;
use crate::core::DefaultLayout;
use crate::core::OperationDirection;
use crate::core::ResizeStep;
use crate::core::Sizing;
use crate::core::TransparencyAlpha;
use crate::core::WindowKind;
use komorebi_themes::colour::Rgb;

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

    let ActionPreparation::Prepared(prepared) = state.prepare(&request, &context(), Instant::now())
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
    state.snapshot.configuration.resize_step = ResizeStep::new(73).expect("test step is positive");
    let request = InvokeAction {
        invocation_id: invocation(12),
        expected_state: stamp(10),
        action: BuiltinAction::ResizeWindowByStep {
            axis: Axis::Horizontal,
            sizing: Sizing::Decrease,
        },
        confirmation: None,
    };

    let ActionPreparation::Prepared(prepared) = state.prepare(&request, &context(), Instant::now())
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

    let ActionPreparation::Prepared(prepared) = state.prepare(&request, &context(), Instant::now())
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
    let ActionPreparation::Prepared(prepared) = state.prepare(&toggle, &context(), Instant::now())
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
fn border_configuration_resolves_to_exact_typed_effects() {
    let cases = [
        (
            BuiltinAction::SetBorderWidth {
                width: BorderWidth::new(-50),
            },
            NativeEffect::SetBorderWidth {
                width: BorderWidth::new(-50),
            },
        ),
        (
            BuiltinAction::SetBorderOffset {
                offset: BorderOffset::new(50),
            },
            NativeEffect::SetBorderOffset {
                offset: BorderOffset::new(50),
            },
        ),
        (
            BuiltinAction::SetBorderStyle {
                style: BorderStyle::Rounded,
            },
            NativeEffect::SetBorderStyle {
                style: BorderStyle::Rounded,
            },
        ),
        (
            BuiltinAction::SetBorderImplementation {
                implementation: BorderImplementation::Windows,
            },
            NativeEffect::SetBorderImplementation {
                implementation: BorderImplementation::Windows,
            },
        ),
        (
            BuiltinAction::SetBorderColour {
                window_kind: WindowKind::Stack,
                colour: Rgb::new(1, 2, 3),
            },
            NativeEffect::SetBorderColour {
                window_kind: WindowKind::Stack,
                colour: Rgb::new(1, 2, 3),
            },
        ),
    ];

    for (action, effect) in cases {
        let state = live_state();
        let request = InvokeAction {
            invocation_id: invocation(16),
            expected_state: stamp(10),
            action,
            confirmation: None,
        };
        let ActionPreparation::Prepared(prepared) =
            state.prepare(&request, &context(), Instant::now())
        else {
            panic!("border configuration should prepare");
        };
        assert_eq!(
            prepared.effects,
            vec![PlannedEffect {
                id: EffectId::new(0),
                effect,
            }]
        );
    }
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
