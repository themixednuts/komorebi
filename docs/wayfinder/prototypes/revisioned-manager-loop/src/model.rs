use std::collections::BTreeMap;
use std::sync::Arc;

use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Revision(pub u64);

impl Revision {
    fn next(self) -> Self {
        Self(self.0 + 1)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct InputId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct EffectId {
    pub revision: Revision,
    pub ordinal: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct WindowId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct WorkspaceId(pub u8);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Geometry {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct SurfaceFrame {
    pub geometry: Geometry,
    pub visible: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum EffectOutcome {
    Applied,
    Rejected,
    TimedOut,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum WindowPresence {
    Present,
    Destroyed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct WindowState {
    pub workspace: WorkspaceId,
    pub intended_geometry: Geometry,
    pub observed_geometry: Geometry,
    pub presence: WindowPresence,
    pub observation_uncertain: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ShellState {
    pub intended: SurfaceFrame,
    pub observed: SurfaceFrame,
    pub degraded: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum ShellPurpose {
    Apply,
    Restore,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum NativeEffect {
    FocusWindow {
        id: EffectId,
        window: WindowId,
    },
    MoveWindow {
        id: EffectId,
        window: WindowId,
        target: Geometry,
    },
    SetShellSurface {
        id: EffectId,
        purpose: ShellPurpose,
        target: SurfaceFrame,
        restore: SurfaceFrame,
    },
}

impl NativeEffect {
    pub const fn id(&self) -> EffectId {
        match self {
            Self::FocusWindow { id, .. }
            | Self::MoveWindow { id, .. }
            | Self::SetShellSurface { id, .. } => *id,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum EffectBoundary {
    FocusWindow,
    MoveWindow,
    ShellSurface,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum Command {
    FocusWorkspace { workspace: WorkspaceId },
    MoveWindow { window: WindowId, target: Geometry },
    SetShellSurface { target: SurfaceFrame },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum PlatformObservation {
    ForegroundWindow {
        window: Option<WindowId>,
    },
    WindowGeometry {
        window: WindowId,
        geometry: Geometry,
    },
    WindowDestroyed {
        window: WindowId,
    },
    WindowUnavailable {
        window: WindowId,
        reason: String,
    },
    ShellSurface {
        frame: SurfaceFrame,
    },
    ShellSurfaceUnavailable {
        reason: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum ManagerInput {
    Command(Command),
    Observation(PlatformObservation),
    EffectReported {
        effect: EffectId,
        outcome: EffectOutcome,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct InputEnvelope {
    pub id: InputId,
    pub expected_revision: Option<Revision>,
    pub input: ManagerInput,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum Rejection {
    StaleRevision {
        expected: Revision,
        actual: Revision,
    },
    UnknownWorkspace(WorkspaceId),
    UnknownWindow(WindowId),
    DestroyedWindow(WindowId),
    UnknownEffect(EffectId),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum Acknowledgement {
    Committed {
        input: InputId,
        revision: Revision,
        effects: Vec<EffectId>,
    },
    Rejected {
        input: InputId,
        at_revision: Revision,
        reason: Rejection,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum CommittedFact {
    WorkspaceFocusRequested {
        workspace: WorkspaceId,
        window: WindowId,
    },
    WindowMoveRequested {
        window: WindowId,
        target: Geometry,
    },
    ShellSurfaceRequested {
        target: SurfaceFrame,
    },
    EffectObserved {
        effect: EffectId,
        outcome: EffectOutcome,
    },
    PlatformObserved(PlatformObservation),
    ReconciliationPlanned {
        effect: EffectId,
    },
    ShellCompensationPlanned {
        effect: EffectId,
        restore: SurfaceFrame,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CommittedEvent {
    pub revision: Revision,
    pub cause: InputId,
    pub fact: CommittedFact,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AuthoritativeState {
    pub revision: Revision,
    pub active_workspace: WorkspaceId,
    pub intended_focus: Option<WindowId>,
    pub observed_foreground: Option<WindowId>,
    pub windows: BTreeMap<WindowId, WindowState>,
    pub shell: ShellState,
    pub pending_effects: BTreeMap<EffectId, NativeEffect>,
}

impl AuthoritativeState {
    pub fn fixture(window_one: Geometry, window_two: Geometry, shell: SurfaceFrame) -> Self {
        Self {
            revision: Revision(0),
            active_workspace: WorkspaceId(1),
            intended_focus: Some(WindowId(1)),
            observed_foreground: Some(WindowId(1)),
            windows: BTreeMap::from([
                (
                    WindowId(1),
                    WindowState {
                        workspace: WorkspaceId(1),
                        intended_geometry: window_one,
                        observed_geometry: window_one,
                        presence: WindowPresence::Present,
                        observation_uncertain: false,
                    },
                ),
                (
                    WindowId(2),
                    WindowState {
                        workspace: WorkspaceId(2),
                        intended_geometry: window_two,
                        observed_geometry: window_two,
                        presence: WindowPresence::Present,
                        observation_uncertain: false,
                    },
                ),
            ]),
            shell: ShellState {
                intended: shell,
                observed: shell,
                degraded: false,
            },
            pending_effects: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlannedTransition {
    pub next: AuthoritativeState,
    pub acknowledgement: Acknowledgement,
    pub events: Vec<CommittedEvent>,
    pub effects: Vec<NativeEffect>,
}

fn effect_id(revision: Revision, ordinal: usize) -> EffectId {
    EffectId {
        revision,
        ordinal: u16::try_from(ordinal).expect("prototype effect plans fit in u16"),
    }
}

fn reject(state: &AuthoritativeState, input: InputId, reason: Rejection) -> PlannedTransition {
    PlannedTransition {
        next: state.clone(),
        acknowledgement: Acknowledgement::Rejected {
            input,
            at_revision: state.revision,
            reason,
        },
        events: Vec::new(),
        effects: Vec::new(),
    }
}

fn commit(
    mut next: AuthoritativeState,
    cause: InputId,
    facts: Vec<CommittedFact>,
    effects: Vec<NativeEffect>,
) -> PlannedTransition {
    next.revision = next.revision.next();
    let revision = next.revision;
    for effect in &effects {
        next.pending_effects.insert(effect.id(), effect.clone());
    }
    let effect_ids = effects.iter().map(NativeEffect::id).collect();

    PlannedTransition {
        next,
        acknowledgement: Acknowledgement::Committed {
            input: cause,
            revision,
            effects: effect_ids,
        },
        events: facts
            .into_iter()
            .map(|fact| CommittedEvent {
                revision,
                cause,
                fact,
            })
            .collect(),
        effects,
    }
}

pub fn plan_transition(state: &AuthoritativeState, envelope: &InputEnvelope) -> PlannedTransition {
    if let Some(expected) = envelope.expected_revision
        && expected != state.revision
    {
        return reject(
            state,
            envelope.id,
            Rejection::StaleRevision {
                expected,
                actual: state.revision,
            },
        );
    }

    match &envelope.input {
        ManagerInput::Command(command) => plan_command(state, envelope.id, command),
        ManagerInput::Observation(observation) => plan_observation(state, envelope.id, observation),
        ManagerInput::EffectReported { effect, outcome } => {
            plan_effect_outcome(state, envelope.id, *effect, *outcome)
        }
    }
}

fn plan_command(
    state: &AuthoritativeState,
    cause: InputId,
    command: &Command,
) -> PlannedTransition {
    let revision = state.revision.next();
    let mut next = state.clone();

    match command {
        Command::FocusWorkspace { workspace } => {
            let Some((&window, _)) = state.windows.iter().find(|(_, candidate)| {
                candidate.workspace == *workspace && candidate.presence == WindowPresence::Present
            }) else {
                return reject(state, cause, Rejection::UnknownWorkspace(*workspace));
            };

            next.active_workspace = *workspace;
            next.intended_focus = Some(window);
            let effect = NativeEffect::FocusWindow {
                id: effect_id(revision, 0),
                window,
            };
            commit(
                next,
                cause,
                vec![CommittedFact::WorkspaceFocusRequested {
                    workspace: *workspace,
                    window,
                }],
                vec![effect],
            )
        }
        Command::MoveWindow { window, target } => {
            let Some(current) = next.windows.get_mut(window) else {
                return reject(state, cause, Rejection::UnknownWindow(*window));
            };
            if current.presence == WindowPresence::Destroyed {
                return reject(state, cause, Rejection::DestroyedWindow(*window));
            }
            current.intended_geometry = *target;
            let effect = NativeEffect::MoveWindow {
                id: effect_id(revision, 0),
                window: *window,
                target: *target,
            };
            commit(
                next,
                cause,
                vec![CommittedFact::WindowMoveRequested {
                    window: *window,
                    target: *target,
                }],
                vec![effect],
            )
        }
        Command::SetShellSurface { target } => {
            let restore = state.shell.observed;
            next.shell.intended = *target;
            next.shell.degraded = false;
            let effect = NativeEffect::SetShellSurface {
                id: effect_id(revision, 0),
                purpose: ShellPurpose::Apply,
                target: *target,
                restore,
            };
            commit(
                next,
                cause,
                vec![CommittedFact::ShellSurfaceRequested { target: *target }],
                vec![effect],
            )
        }
    }
}

fn plan_observation(
    state: &AuthoritativeState,
    cause: InputId,
    observation: &PlatformObservation,
) -> PlannedTransition {
    let revision = state.revision.next();
    let mut next = state.clone();
    let mut effects = Vec::new();
    let mut facts = vec![CommittedFact::PlatformObserved(observation.clone())];

    match observation {
        PlatformObservation::ForegroundWindow { window } => {
            next.observed_foreground = *window;
            if let Some(intended) = next.intended_focus
                && *window != Some(intended)
                && next
                    .windows
                    .get(&intended)
                    .is_some_and(|candidate| candidate.presence == WindowPresence::Present)
            {
                let effect = NativeEffect::FocusWindow {
                    id: effect_id(revision, effects.len()),
                    window: intended,
                };
                facts.push(CommittedFact::ReconciliationPlanned {
                    effect: effect.id(),
                });
                effects.push(effect);
            }
        }
        PlatformObservation::WindowGeometry { window, geometry } => {
            let Some(current) = next.windows.get_mut(window) else {
                return reject(state, cause, Rejection::UnknownWindow(*window));
            };
            if current.presence == WindowPresence::Destroyed {
                return reject(state, cause, Rejection::DestroyedWindow(*window));
            }
            current.observed_geometry = *geometry;
            current.observation_uncertain = false;
            if current.intended_geometry != *geometry {
                let effect = NativeEffect::MoveWindow {
                    id: effect_id(revision, effects.len()),
                    window: *window,
                    target: current.intended_geometry,
                };
                facts.push(CommittedFact::ReconciliationPlanned {
                    effect: effect.id(),
                });
                effects.push(effect);
            }
        }
        PlatformObservation::WindowDestroyed { window } => {
            let Some(current) = next.windows.get_mut(window) else {
                return reject(state, cause, Rejection::UnknownWindow(*window));
            };
            current.presence = WindowPresence::Destroyed;
            current.observation_uncertain = false;
            next.pending_effects.retain(|_, effect| match effect {
                NativeEffect::FocusWindow {
                    window: affected, ..
                }
                | NativeEffect::MoveWindow {
                    window: affected, ..
                } => affected != window,
                NativeEffect::SetShellSurface { .. } => true,
            });
            if next.intended_focus == Some(*window) {
                next.intended_focus = None;
            }
            if next.observed_foreground == Some(*window) {
                next.observed_foreground = None;
            }
        }
        PlatformObservation::WindowUnavailable { window, .. } => {
            let Some(current) = next.windows.get_mut(window) else {
                return reject(state, cause, Rejection::UnknownWindow(*window));
            };
            current.observation_uncertain = true;
        }
        PlatformObservation::ShellSurface { frame } => {
            next.shell.observed = *frame;
            next.shell.degraded = next.shell.intended != *frame;
        }
        PlatformObservation::ShellSurfaceUnavailable { .. } => {
            next.shell.degraded = true;
        }
    }

    commit(next, cause, facts, effects)
}

fn plan_effect_outcome(
    state: &AuthoritativeState,
    cause: InputId,
    effect_id_value: EffectId,
    outcome: EffectOutcome,
) -> PlannedTransition {
    let Some(effect) = state.pending_effects.get(&effect_id_value).cloned() else {
        return reject(state, cause, Rejection::UnknownEffect(effect_id_value));
    };

    let revision = state.revision.next();
    let mut next = state.clone();
    next.pending_effects.remove(&effect_id_value);
    let mut effects = Vec::new();
    let mut facts = vec![CommittedFact::EffectObserved {
        effect: effect_id_value,
        outcome,
    }];

    if let NativeEffect::SetShellSurface {
        purpose: ShellPurpose::Apply,
        restore,
        ..
    } = effect
    {
        match outcome {
            EffectOutcome::Applied => {}
            EffectOutcome::Rejected => {
                next.shell.intended = restore;
                next.shell.degraded = next.shell.observed != restore;
            }
            EffectOutcome::TimedOut | EffectOutcome::Unknown => {
                next.shell.intended = restore;
                let compensation = NativeEffect::SetShellSurface {
                    id: effect_id(revision, 0),
                    purpose: ShellPurpose::Restore,
                    target: restore,
                    restore,
                };
                facts.push(CommittedFact::ShellCompensationPlanned {
                    effect: compensation.id(),
                    restore,
                });
                effects.push(compensation);
            }
        }
    } else if let NativeEffect::SetShellSurface {
        purpose: ShellPurpose::Restore,
        restore,
        ..
    } = effect
    {
        if outcome != EffectOutcome::Applied {
            next.shell.degraded = true;
        } else {
            next.shell.intended = restore;
        }
    }

    commit(next, cause, facts, effects)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct OwnerRecord {
    pub inputs: Vec<InputEnvelope>,
    pub acknowledgements: Vec<Acknowledgement>,
    pub events: Vec<CommittedEvent>,
}

#[derive(Clone, Debug)]
pub struct OrderedOwner {
    initial: AuthoritativeState,
    state: AuthoritativeState,
    record: OwnerRecord,
    next_input: u64,
}

impl OrderedOwner {
    pub fn new(initial: AuthoritativeState) -> Self {
        Self {
            state: initial.clone(),
            initial,
            record: OwnerRecord {
                inputs: Vec::new(),
                acknowledgements: Vec::new(),
                events: Vec::new(),
            },
            next_input: 1,
        }
    }

    pub fn submit(
        &mut self,
        input: ManagerInput,
        expected_revision: Option<Revision>,
    ) -> (Acknowledgement, Vec<NativeEffect>) {
        let envelope = InputEnvelope {
            id: InputId(self.next_input),
            expected_revision,
            input,
        };
        self.next_input += 1;
        let transition = plan_transition(&self.state, &envelope);
        self.state = transition.next;
        self.record.inputs.push(envelope);
        self.record
            .acknowledgements
            .push(transition.acknowledgement.clone());
        self.record.events.extend(transition.events);
        (transition.acknowledgement, transition.effects)
    }

    pub fn snapshot(&self) -> Arc<AuthoritativeState> {
        Arc::new(self.state.clone())
    }

    pub const fn record(&self) -> &OwnerRecord {
        &self.record
    }

    pub fn replay_matches(&self) -> bool {
        let mut replay = Self::new(self.initial.clone());
        for envelope in &self.record.inputs {
            let transition = plan_transition(&replay.state, envelope);
            replay.state = transition.next;
            replay.record.inputs.push(envelope.clone());
            replay
                .record
                .acknowledgements
                .push(transition.acknowledgement);
            replay.record.events.extend(transition.events);
            replay.next_input = replay.next_input.max(envelope.id.0 + 1);
        }

        replay.state == self.state
            && replay.record.acknowledgements == self.record.acknowledgements
            && replay.record.events == self.record.events
    }
}
