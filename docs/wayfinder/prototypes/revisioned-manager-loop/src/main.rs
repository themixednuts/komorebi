mod model;
mod native;

use std::collections::{BTreeMap, VecDeque};
use std::error::Error;

use model::{
    Acknowledgement, AuthoritativeState, Command, CommittedFact, EffectBoundary, EffectOutcome,
    Geometry, ManagerInput, NativeEffect, OrderedOwner, PlatformObservation, Rejection, Revision,
    ShellPurpose, SurfaceFrame, WindowId, WindowPresence, WorkspaceId,
};
use native::{
    NativePorts, ObservedSurface, ObservedWindow, ScriptedNative, ShellSurfaceHost, Win32Probe,
    WindowSystem,
};
use serde::Serialize;

const WINDOW_ONE: WindowId = WindowId(1);
const WINDOW_TWO: WindowId = WindowId(2);

fn main() -> Result<(), Box<dyn Error>> {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "fake".to_owned());
    match mode.as_str() {
        "fake" => println!("{}", serde_json::to_string_pretty(&run_fake_matrix())?),
        "live" => println!("{}", serde_json::to_string_pretty(&run_live()?)?),
        _ => return Err(format!("unknown mode {mode}; use fake or live").into()),
    }
    Ok(())
}

#[derive(Debug, Serialize)]
struct EffectExecution {
    effect: NativeEffect,
    outcome: EffectOutcome,
    outcome_acknowledgement: Acknowledgement,
    observation_acknowledgement: Acknowledgement,
    follow_up_effects: Vec<NativeEffect>,
}

fn execute_effect(
    owner: &mut OrderedOwner,
    native: &mut impl NativePorts,
    effect: NativeEffect,
) -> EffectExecution {
    let outcome = match effect {
        NativeEffect::FocusWindow { window, .. } => native.focus_window(window),
        NativeEffect::MoveWindow { window, target, .. } => native.move_window(window, target),
        NativeEffect::SetShellSurface { target, .. } => native.set_surface(target),
    };
    let (outcome_acknowledgement, mut follow_up_effects) = owner.submit(
        ManagerInput::EffectReported {
            effect: effect.id(),
            outcome,
        },
        None,
    );

    let observation = match effect {
        NativeEffect::FocusWindow { .. } => PlatformObservation::ForegroundWindow {
            window: native.observe_foreground(),
        },
        NativeEffect::MoveWindow { window, .. } => match native.observe_window(window) {
            ObservedWindow::Present(geometry) => {
                PlatformObservation::WindowGeometry { window, geometry }
            }
            ObservedWindow::Destroyed => PlatformObservation::WindowDestroyed { window },
            ObservedWindow::Unknown(reason) => {
                PlatformObservation::WindowUnavailable { window, reason }
            }
        },
        NativeEffect::SetShellSurface { .. } => match native.observe_surface() {
            ObservedSurface::Present(frame) => PlatformObservation::ShellSurface { frame },
            ObservedSurface::Unknown(reason) => {
                PlatformObservation::ShellSurfaceUnavailable { reason }
            }
        },
    };
    let (observation_acknowledgement, observation_effects) =
        owner.submit(ManagerInput::Observation(observation), None);
    follow_up_effects.extend(observation_effects);

    EffectExecution {
        effect,
        outcome,
        outcome_acknowledgement,
        observation_acknowledgement,
        follow_up_effects,
    }
}

#[derive(Debug, Serialize)]
struct FakeReport {
    question: &'static str,
    cases: Vec<FakeCase>,
    stale_revision_acknowledgement: Acknowledgement,
    stale_revision_changed_state: bool,
    all_cases_passed: bool,
}

#[derive(Debug, Serialize)]
struct FakeCase {
    boundary: EffectBoundary,
    injected_outcome: EffectOutcome,
    executions: Vec<EffectExecution>,
    input_count: usize,
    acknowledgement_count: usize,
    replay_matches: bool,
    events_are_revisioned: bool,
    reconciliation_planned: bool,
    compensation_planned: bool,
    exact_shell_restore: Option<bool>,
    passed: bool,
}

fn fixture() -> (
    AuthoritativeState,
    BTreeMap<WindowId, Geometry>,
    SurfaceFrame,
) {
    let window_one = Geometry {
        x: 100,
        y: 150,
        width: 360,
        height: 220,
    };
    let window_two = Geometry {
        x: 500,
        y: 150,
        width: 360,
        height: 220,
    };
    let shell = SurfaceFrame {
        geometry: Geometry {
            x: 260,
            y: 70,
            width: 440,
            height: 64,
        },
        visible: false,
    };
    (
        AuthoritativeState::fixture(window_one, window_two, shell),
        BTreeMap::from([(WINDOW_ONE, window_one), (WINDOW_TWO, window_two)]),
        shell,
    )
}

fn command_for(boundary: EffectBoundary) -> Command {
    match boundary {
        EffectBoundary::FocusWindow => Command::FocusWorkspace {
            workspace: WorkspaceId(2),
        },
        EffectBoundary::MoveWindow => Command::MoveWindow {
            window: WINDOW_ONE,
            target: Geometry {
                x: 180,
                y: 230,
                width: 420,
                height: 260,
            },
        },
        EffectBoundary::ShellSurface => Command::SetShellSurface {
            target: SurfaceFrame {
                geometry: Geometry {
                    x: 300,
                    y: 90,
                    width: 480,
                    height: 72,
                },
                visible: true,
            },
        },
    }
}

fn run_fake_matrix() -> FakeReport {
    let boundaries = [
        EffectBoundary::FocusWindow,
        EffectBoundary::MoveWindow,
        EffectBoundary::ShellSurface,
    ];
    let outcomes = [
        EffectOutcome::Applied,
        EffectOutcome::Rejected,
        EffectOutcome::TimedOut,
        EffectOutcome::Unknown,
    ];
    let cases = boundaries
        .into_iter()
        .flat_map(|boundary| {
            outcomes
                .into_iter()
                .map(move |outcome| run_fake_case(boundary, outcome))
        })
        .collect::<Vec<_>>();

    let (initial, _, _) = fixture();
    let mut stale_owner = OrderedOwner::new(initial);
    let stale_before = stale_owner.snapshot();
    let (stale_revision_acknowledgement, effects) = stale_owner.submit(
        ManagerInput::Command(Command::FocusWorkspace {
            workspace: WorkspaceId(2),
        }),
        Some(Revision(99)),
    );
    let stale_revision_changed_state = *stale_before != *stale_owner.snapshot();
    assert!(effects.is_empty(), "stale command produced native work");

    FakeReport {
        question: "Does one revisioned owner converge safely when every native effect outcome is explicit?",
        all_cases_passed: cases.iter().all(|case| case.passed)
            && !stale_revision_changed_state
            && matches!(
                stale_revision_acknowledgement,
                Acknowledgement::Rejected {
                    reason: Rejection::StaleRevision { .. },
                    ..
                }
            ),
        cases,
        stale_revision_acknowledgement,
        stale_revision_changed_state,
    }
}

fn run_fake_case(boundary: EffectBoundary, outcome: EffectOutcome) -> FakeCase {
    let (initial, windows, shell) = fixture();
    let scripted_outcomes = if boundary == EffectBoundary::ShellSurface
        && matches!(outcome, EffectOutcome::TimedOut | EffectOutcome::Unknown)
    {
        vec![
            (boundary, outcome),
            (EffectBoundary::ShellSurface, EffectOutcome::Applied),
        ]
    } else {
        vec![(boundary, outcome)]
    };
    let mut native = ScriptedNative::new(windows, shell, scripted_outcomes);
    let mut owner = OrderedOwner::new(initial);
    let revision = owner.snapshot().revision;
    let (_, effects) = owner.submit(ManagerInput::Command(command_for(boundary)), Some(revision));
    let mut pending = VecDeque::from(effects);
    let mut executions = Vec::new();

    if let Some(effect) = pending.pop_front() {
        let execution = execute_effect(&mut owner, &mut native, effect);
        if boundary == EffectBoundary::ShellSurface
            && execution.follow_up_effects.iter().any(|effect| {
                matches!(
                    effect,
                    NativeEffect::SetShellSurface {
                        purpose: ShellPurpose::Restore,
                        ..
                    }
                )
            })
        {
            pending.extend(execution.follow_up_effects.clone());
        }
        executions.push(execution);
    }
    if let Some(compensation) = pending.pop_front() {
        executions.push(execute_effect(&mut owner, &mut native, compensation));
    }

    let record = owner.record();
    let reconciliation_planned = record
        .events
        .iter()
        .any(|event| matches!(event.fact, CommittedFact::ReconciliationPlanned { .. }));
    let compensation_planned = record
        .events
        .iter()
        .any(|event| matches!(event.fact, CommittedFact::ShellCompensationPlanned { .. }));
    let exact_shell_restore = (boundary == EffectBoundary::ShellSurface
        && outcome != EffectOutcome::Applied)
        .then(|| {
            native.observe_surface() == ObservedSurface::Present(shell)
                && owner.snapshot().shell.intended == shell
        });
    let input_count = record.inputs.len();
    let acknowledgement_count = record.acknowledgements.len();
    let replay_matches = owner.replay_matches();
    let events_are_revisioned = events_are_revisioned(&owner);
    let recovery_is_explicit = match (boundary, outcome) {
        (EffectBoundary::FocusWindow | EffectBoundary::MoveWindow, EffectOutcome::Rejected) => {
            reconciliation_planned
        }
        (EffectBoundary::ShellSurface, EffectOutcome::TimedOut | EffectOutcome::Unknown) => {
            compensation_planned && exact_shell_restore == Some(true)
        }
        (EffectBoundary::ShellSurface, EffectOutcome::Rejected) => {
            exact_shell_restore == Some(true)
        }
        _ => true,
    };

    FakeCase {
        boundary,
        injected_outcome: outcome,
        executions,
        input_count,
        acknowledgement_count,
        replay_matches,
        events_are_revisioned,
        reconciliation_planned,
        compensation_planned,
        exact_shell_restore,
        passed: input_count == acknowledgement_count
            && replay_matches
            && events_are_revisioned
            && recovery_is_explicit,
    }
}

fn events_are_revisioned(owner: &OrderedOwner) -> bool {
    owner.record().events.iter().all(|event| {
        event.revision.0 > 0
            && owner
                .record()
                .inputs
                .iter()
                .any(|input| input.id == event.cause)
    })
}

#[derive(Debug, Serialize)]
struct LiveReport {
    question: &'static str,
    scenarios: Vec<LiveScenario>,
    effect_executions: Vec<EffectExecution>,
    final_revision: Revision,
    input_count: usize,
    acknowledgement_count: usize,
    replay_matches: bool,
    events_are_revisioned: bool,
    timed_out_outcome_observable_with_same_thread_test_windows: bool,
    divergences: Vec<String>,
    all_accepted_cases_passed: bool,
}

#[derive(Debug, Serialize)]
struct LiveScenario {
    name: &'static str,
    passed: bool,
    evidence: String,
}

fn run_live() -> Result<LiveReport, Box<dyn Error>> {
    let mut native = Win32Probe::create()?;
    let window_one = native.window_geometry(WINDOW_ONE)?;
    let window_two = native.window_geometry(WINDOW_TWO)?;
    let initial_shell = native.shell_frame()?;
    let mut owner = OrderedOwner::new(AuthoritativeState::fixture(
        window_one,
        window_two,
        initial_shell,
    ));
    let mut scenarios = Vec::new();
    let mut effect_executions = Vec::new();
    let mut divergences = Vec::new();

    let focus_effects = submit_command(
        &mut owner,
        Command::FocusWorkspace {
            workspace: WorkspaceId(2),
        },
    );
    let focus_execution =
        execute_required_effect(&mut owner, &mut native, focus_effects, "workspace focus")?;
    let focus_passed = owner.snapshot().observed_foreground == Some(WINDOW_TWO);
    scenarios.push(LiveScenario {
        name: "workspace focus",
        passed: focus_passed,
        evidence: format!(
            "effect={:?}, observed_foreground={:?}",
            focus_execution.outcome,
            owner.snapshot().observed_foreground
        ),
    });
    effect_executions.push(focus_execution);

    let move_target = Geometry {
        x: window_one.x + 45,
        y: window_one.y + 35,
        width: window_one.width + 20,
        height: window_one.height + 10,
    };
    let move_effects = submit_command(
        &mut owner,
        Command::MoveWindow {
            window: WINDOW_ONE,
            target: move_target,
        },
    );
    let move_execution =
        execute_required_effect(&mut owner, &mut native, move_effects, "window movement")?;
    let move_passed = matches!(
        native.observe_window(WINDOW_ONE),
        ObservedWindow::Present(observed) if observed == move_target
    );
    scenarios.push(LiveScenario {
        name: "window movement",
        passed: move_passed,
        evidence: format!(
            "effect={:?}, target={move_target:?}, observed={:?}",
            move_execution.outcome,
            native.observe_window(WINDOW_ONE)
        ),
    });
    effect_executions.push(move_execution);

    let external_geometry = Geometry {
        x: move_target.x + 75,
        y: move_target.y + 55,
        ..move_target
    };
    native.external_move(WINDOW_ONE, external_geometry)?;
    let (_, reconciliation_effects) = owner.submit(
        ManagerInput::Observation(PlatformObservation::WindowGeometry {
            window: WINDOW_ONE,
            geometry: external_geometry,
        }),
        None,
    );
    let reconciliation_planned = reconciliation_effects.iter().any(|effect| {
        matches!(
            effect,
            NativeEffect::MoveWindow {
                window: WINDOW_ONE,
                target,
                ..
            } if *target == move_target
        )
    });
    let reconciliation_execution = execute_required_effect(
        &mut owner,
        &mut native,
        reconciliation_effects,
        "external movement reconciliation",
    )?;
    let external_move_passed = reconciliation_planned
        && matches!(
            native.observe_window(WINDOW_ONE),
            ObservedWindow::Present(observed) if observed == move_target
        );
    scenarios.push(LiveScenario {
        name: "externally moved HWND",
        passed: external_move_passed,
        evidence: format!(
            "external={external_geometry:?}, reconciliation_planned={reconciliation_planned}, final={:?}",
            native.observe_window(WINDOW_ONE)
        ),
    });
    effect_executions.push(reconciliation_execution);

    native.external_destroy(WINDOW_TWO)?;
    let (_, destroyed_effects) = owner.submit(
        ManagerInput::Observation(PlatformObservation::WindowDestroyed { window: WINDOW_TWO }),
        None,
    );
    let destroyed_passed = destroyed_effects.is_empty()
        && owner
            .snapshot()
            .windows
            .get(&WINDOW_TWO)
            .is_some_and(|window| window.presence == WindowPresence::Destroyed)
        && !owner.snapshot().pending_effects.values().any(|effect| {
            matches!(
                effect,
                NativeEffect::FocusWindow {
                    window: WINDOW_TWO,
                    ..
                } | NativeEffect::MoveWindow {
                    window: WINDOW_TWO,
                    ..
                }
            )
        });
    scenarios.push(LiveScenario {
        name: "externally destroyed HWND",
        passed: destroyed_passed,
        evidence: format!(
            "native={:?}, model_presence={:?}, follow_up_effects={}",
            native.observe_window(WINDOW_TWO),
            owner
                .snapshot()
                .windows
                .get(&WINDOW_TWO)
                .map(|window| window.presence),
            destroyed_effects.len()
        ),
    });

    let shell_target = SurfaceFrame {
        geometry: Geometry {
            x: initial_shell.geometry.x + 30,
            y: initial_shell.geometry.y + 30,
            width: initial_shell.geometry.width + 40,
            height: initial_shell.geometry.height + 8,
        },
        visible: true,
    };
    let shell_effects = submit_command(
        &mut owner,
        Command::SetShellSurface {
            target: shell_target,
        },
    );
    let shell_execution = execute_required_effect(
        &mut owner,
        &mut native,
        shell_effects,
        "manager-owned shell surface",
    )?;
    let shell_apply_passed = native.shell_frame()? == shell_target;
    scenarios.push(LiveScenario {
        name: "manager-owned shell surface",
        passed: shell_apply_passed,
        evidence: format!(
            "effect={:?}, target={shell_target:?}, observed={:?}",
            shell_execution.outcome,
            native.shell_frame()?
        ),
    });
    effect_executions.push(shell_execution);

    let restore_effects = submit_command(
        &mut owner,
        Command::SetShellSurface {
            target: initial_shell,
        },
    );
    let restore_execution = execute_required_effect(
        &mut owner,
        &mut native,
        restore_effects,
        "shell exact restoration",
    )?;
    let shell_restore_passed = native.shell_frame()? == initial_shell;
    scenarios.push(LiveScenario {
        name: "shell exact restoration",
        passed: shell_restore_passed,
        evidence: format!(
            "effect={:?}, captured={initial_shell:?}, restored={:?}",
            restore_execution.outcome,
            native.shell_frame()?
        ),
    });
    effect_executions.push(restore_execution);

    let before_stale = owner.snapshot();
    let (stale_acknowledgement, stale_effects) = owner.submit(
        ManagerInput::Command(Command::MoveWindow {
            window: WINDOW_ONE,
            target: external_geometry,
        }),
        Some(Revision(0)),
    );
    let stale_passed = stale_effects.is_empty()
        && *before_stale == *owner.snapshot()
        && matches!(
            stale_acknowledgement,
            Acknowledgement::Rejected {
                reason: Rejection::StaleRevision { .. },
                ..
            }
        );
    scenarios.push(LiveScenario {
        name: "stale command rejection",
        passed: stale_passed,
        evidence: format!("acknowledgement={stale_acknowledgement:?}"),
    });

    if let Some(error) = native.take_last_error() {
        divergences.push(error);
    }
    for scenario in &scenarios {
        if !scenario.passed {
            divergences.push(format!(
                "{} did not pass: {}",
                scenario.name, scenario.evidence
            ));
        }
    }
    let input_count = owner.record().inputs.len();
    let acknowledgement_count = owner.record().acknowledgements.len();
    let replay_matches = owner.replay_matches();
    let events_are_revisioned = events_are_revisioned(&owner);
    if input_count != acknowledgement_count {
        divergences.push(format!(
            "input ledger has {input_count} inputs and {acknowledgement_count} acknowledgements"
        ));
    }
    if !replay_matches {
        divergences.push(
            "deterministic replay did not reproduce state, events, and acknowledgements".to_owned(),
        );
    }
    if !events_are_revisioned {
        divergences
            .push("one or more committed events lacked a valid revision or cause".to_owned());
    }

    Ok(LiveReport {
        question: "Does the accepted revisioned state-and-effect model survive temporary live Win32 windows?",
        all_accepted_cases_passed: scenarios.iter().all(|scenario| scenario.passed)
            && input_count == acknowledgement_count
            && replay_matches
            && events_are_revisioned
            && divergences.is_empty(),
        scenarios,
        effect_executions,
        final_revision: owner.snapshot().revision,
        input_count,
        acknowledgement_count,
        replay_matches,
        events_are_revisioned,
        timed_out_outcome_observable_with_same_thread_test_windows: false,
        divergences,
    })
}

fn submit_command(owner: &mut OrderedOwner, command: Command) -> Vec<NativeEffect> {
    let expected = owner.snapshot().revision;
    let (acknowledgement, effects) = owner.submit(ManagerInput::Command(command), Some(expected));
    assert!(
        matches!(acknowledgement, Acknowledgement::Committed { .. }),
        "valid prototype command was rejected"
    );
    effects
}

fn execute_required_effect(
    owner: &mut OrderedOwner,
    native: &mut impl NativePorts,
    mut effects: Vec<NativeEffect>,
    scenario: &str,
) -> Result<EffectExecution, Box<dyn Error>> {
    if effects.len() != 1 {
        return Err(format!(
            "{scenario} expected one effect but planned {}",
            effects.len()
        )
        .into());
    }
    Ok(execute_effect(owner, native, effects.remove(0)))
}
