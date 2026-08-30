mod availability;
mod choices;
mod policy;

use crate::core::DefaultLayout;
use crate::core::OperationDirection;
use crate::core::TransparencyAlpha;

use super::DirectionSet;
use super::builtin::BuiltinActionKind;
use super::builtin::WorkspaceName;
use super::configuration::ConfigurationSnapshot;
use super::definition::ActionDefinition;
use super::definition::layout_name;
use super::id::ActionId;
use super::id::WindowId;
use super::index::MonitorIndex;
use super::index::WorkspaceIndex;
use komorebi_protocol::ManagerEpoch;
use komorebi_protocol::Revision;
use komorebi_protocol::StateStamp;

pub use choices::DynamicParameterChoice;
pub use choices::DynamicParameterChoices;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NamedWorkspaceTarget {
    pub name: WorkspaceName,
    pub monitor: MonitorIndex,
    pub workspace: WorkspaceIndex,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActionSnapshot {
    pub state: StateStamp,
    pub paused: bool,
    pub focused_window: Option<WindowId>,
    pub directional_targets: DirectionSet,
    pub current_layout: DefaultLayout,
    pub configuration: ConfigurationSnapshot,
    pub focused_window_floating: bool,
    pub named_workspaces: Vec<NamedWorkspaceTarget>,
    pub bindings: Vec<BindingHint>,
}

impl ActionSnapshot {
    #[must_use]
    pub fn empty(manager_epoch: ManagerEpoch) -> Self {
        Self {
            state: StateStamp::initial(manager_epoch),
            paused: false,
            focused_window: None,
            directional_targets: DirectionSet::empty(),
            current_layout: DefaultLayout::BSP,
            configuration: ConfigurationSnapshot::default(),
            focused_window_floating: false,
            named_workspaces: Vec::new(),
            bindings: Vec::new(),
        }
    }

    #[must_use]
    pub fn workspace_by_name(
        &self,
        name: &WorkspaceName,
    ) -> Option<(MonitorIndex, WorkspaceIndex)> {
        self.named_workspaces
            .iter()
            .find(|target| &target.name == name)
            .map(|target| (target.monitor, target.workspace))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BindingHint {
    pub action: ActionId,
    pub trigger: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActionGrants {
    revision: Revision,
    kinds: Vec<BuiltinActionKind>,
}

impl ActionGrants {
    #[must_use]
    pub fn all() -> Self {
        Self {
            revision: Revision::FIRST,
            kinds: BuiltinActionKind::ALL.to_vec(),
        }
    }

    #[must_use]
    pub fn none() -> Self {
        Self {
            revision: Revision::FIRST,
            kinds: Vec::new(),
        }
    }

    #[must_use]
    pub const fn revision(&self) -> Revision {
        self.revision
    }

    #[must_use]
    pub fn contains(&self, kind: BuiltinActionKind) -> bool {
        self.kinds.contains(&kind)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActionAuthority {
    pub grants: ActionGrants,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Unavailability {
    ManagerPaused,
    NoFocusedWindow,
    NoWindowInDirection,
    Unauthorized,
    UnknownWorkspace,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ActionAvailability {
    Available,
    Unavailable(Unavailability),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActionCurrentValue {
    Layout(DefaultLayout),
    Floating(bool),
    TransparencyEnabled(bool),
    TransparencyAlpha(TransparencyAlpha),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActionOffer {
    pub definition: &'static ActionDefinition,
    pub state: StateStamp,
    pub availability: ActionAvailability,
    pub current_value: Option<ActionCurrentValue>,
    pub dynamic_choices: Vec<DynamicParameterChoices>,
    pub bindings: Vec<BindingHint>,
}

#[must_use]
pub fn offers(snapshot: &ActionSnapshot, authority: &ActionAuthority) -> Vec<ActionOffer> {
    BuiltinActionKind::ALL
        .into_iter()
        .map(|kind| offer_kind(snapshot, authority, kind))
        .collect()
}

fn offer_kind(
    snapshot: &ActionSnapshot,
    authority: &ActionAuthority,
    kind: BuiltinActionKind,
) -> ActionOffer {
    let definition = kind.definition();
    let policy = policy::policy(kind);
    let bindings = snapshot
        .bindings
        .iter()
        .filter(|hint| hint.action == definition.id)
        .cloned()
        .collect();
    ActionOffer {
        definition,
        state: snapshot.state,
        availability: availability::availability(snapshot, authority, kind, policy),
        current_value: current_value(snapshot, policy.current_value),
        dynamic_choices: choices::dynamic_choices(snapshot, policy.choices),
        bindings,
    }
}

fn current_value(
    snapshot: &ActionSnapshot,
    source: policy::CurrentValueSource,
) -> Option<ActionCurrentValue> {
    match source {
        policy::CurrentValueSource::Layout => {
            Some(ActionCurrentValue::Layout(snapshot.current_layout))
        }
        policy::CurrentValueSource::Floating => Some(ActionCurrentValue::Floating(
            snapshot.focused_window_floating,
        )),
        policy::CurrentValueSource::TransparencyEnabled => Some(
            ActionCurrentValue::TransparencyEnabled(snapshot.configuration.transparency.enabled),
        ),
        policy::CurrentValueSource::TransparencyAlpha => Some(
            ActionCurrentValue::TransparencyAlpha(snapshot.configuration.transparency.alpha),
        ),
        policy::CurrentValueSource::None => None,
    }
}

#[must_use]
pub fn neighbor_in(snapshot: &ActionSnapshot, direction: OperationDirection) -> bool {
    snapshot.directional_targets.contains(direction)
}

#[must_use]
pub fn current_layout_name(snapshot: &ActionSnapshot) -> &'static str {
    layout_name(snapshot.current_layout)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::ParameterId;
    use crate::action::definition::ParameterDomain;
    use crate::core::TransparencyAlpha;

    fn stamp(revision: u64) -> StateStamp {
        StateStamp::new(
            ManagerEpoch::new([1; 16]).expect("test epoch is non-nil"),
            komorebi_protocol::Revision::try_from(revision).expect("test revision is nonzero"),
        )
    }

    fn live_snapshot() -> ActionSnapshot {
        ActionSnapshot {
            state: stamp(3),
            paused: false,
            focused_window: Some(WindowId::new(1)),
            directional_targets: [OperationDirection::Left].into(),
            current_layout: DefaultLayout::BSP,
            configuration: ConfigurationSnapshot::default(),
            focused_window_floating: false,
            named_workspaces: vec![NamedWorkspaceTarget {
                name: WorkspaceName::parse("chat").unwrap(),
                monitor: MonitorIndex::new(1),
                workspace: WorkspaceIndex::new(2),
            }],
            bindings: vec![BindingHint {
                action: ActionId::FOCUS_WINDOW,
                trigger: "alt+h".to_owned(),
            }],
        }
    }

    fn all_authority() -> ActionAuthority {
        ActionAuthority {
            grants: ActionGrants::all(),
        }
    }

    fn offer(snapshot: &ActionSnapshot, kind: BuiltinActionKind) -> ActionOffer {
        offers(snapshot, &all_authority())
            .into_iter()
            .find(|offer| offer.definition.kind == kind)
            .expect("every built-in kind has an offer")
    }

    #[test]
    fn paused_manager_keeps_actions_discoverable_with_a_reason() {
        let mut snapshot = live_snapshot();
        snapshot.paused = true;
        let catalog = offers(&snapshot, &all_authority());
        assert_eq!(catalog.len(), BuiltinActionKind::ALL.len());
        assert!(catalog.iter().all(|offer| {
            if offer.definition.kind == BuiltinActionKind::TogglePause {
                offer.availability == ActionAvailability::Available
            } else {
                offer.availability == ActionAvailability::Unavailable(Unavailability::ManagerPaused)
            }
        }));
    }

    #[test]
    fn missing_focus_is_explained_instead_of_hidden() {
        let mut snapshot = live_snapshot();
        snapshot.focused_window = None;
        assert_eq!(
            offer(&snapshot, BuiltinActionKind::FocusWindow).availability,
            ActionAvailability::Unavailable(Unavailability::NoFocusedWindow)
        );
    }

    #[test]
    fn layout_offer_projects_the_current_built_in_layout() {
        let snapshot = live_snapshot();
        let layout = offer(&snapshot, BuiltinActionKind::SetWorkspaceLayout);
        assert_eq!(
            layout.current_value,
            Some(ActionCurrentValue::Layout(DefaultLayout::BSP))
        );
        assert_eq!(current_layout_name(&snapshot), "bsp");
        assert_eq!(layout.state, stamp(3));
    }

    #[test]
    fn transparency_setters_expose_their_typed_current_values() {
        let mut snapshot = live_snapshot();
        snapshot.configuration.transparency.enabled = true;
        snapshot.configuration.transparency.alpha = TransparencyAlpha::new(177);

        assert_eq!(
            offer(&snapshot, BuiltinActionKind::SetTransparencyEnabled).current_value,
            Some(ActionCurrentValue::TransparencyEnabled(true))
        );
        assert_eq!(
            offer(&snapshot, BuiltinActionKind::SetTransparencyAlpha).current_value,
            Some(ActionCurrentValue::TransparencyAlpha(
                TransparencyAlpha::new(177)
            ))
        );
    }

    #[test]
    fn directional_choices_expose_only_live_targets() {
        let focus = offer(&live_snapshot(), BuiltinActionKind::FocusWindow);
        assert_eq!(
            focus.dynamic_choices,
            vec![DynamicParameterChoices {
                parameter: ParameterId::DIRECTION,
                choices: vec![DynamicParameterChoice::Direction(OperationDirection::Left)],
            }]
        );
    }

    #[test]
    fn named_workspace_choices_are_typed_action_arguments() {
        let focus = offer(&live_snapshot(), BuiltinActionKind::FocusNamedWorkspace);
        assert_eq!(
            focus.dynamic_choices,
            vec![DynamicParameterChoices {
                parameter: ParameterId::NAME,
                choices: vec![DynamicParameterChoice::WorkspaceName(
                    WorkspaceName::parse("chat").unwrap()
                )],
            }]
        );
    }

    #[test]
    fn promote_window_is_unavailable_without_a_directional_target() {
        let mut snapshot = live_snapshot();
        snapshot.directional_targets = DirectionSet::empty();
        assert_eq!(
            offer(&snapshot, BuiltinActionKind::PromoteWindow).availability,
            ActionAvailability::Unavailable(Unavailability::NoWindowInDirection)
        );
    }

    #[test]
    fn an_empty_named_workspace_domain_is_explained() {
        let mut snapshot = live_snapshot();
        snapshot.named_workspaces.clear();
        let offer = offer(&snapshot, BuiltinActionKind::FocusNamedWorkspace);
        assert_eq!(
            offer.availability,
            ActionAvailability::Unavailable(Unavailability::UnknownWorkspace)
        );
        assert!(offer.dynamic_choices.is_empty());
    }

    #[test]
    fn every_dynamic_choice_matches_a_declared_parameter_domain() {
        for offer in offers(&live_snapshot(), &all_authority()) {
            for dynamic in offer.dynamic_choices {
                let parameter = offer
                    .definition
                    .parameters
                    .iter()
                    .find(|parameter| parameter.id == dynamic.parameter)
                    .expect("dynamic choices reference a declared parameter");
                assert!(dynamic.choices.iter().all(|choice| matches!(
                    (parameter.domain, choice),
                    (
                        ParameterDomain::Direction,
                        DynamicParameterChoice::Direction(_)
                    ) | (
                        ParameterDomain::Name,
                        DynamicParameterChoice::WorkspaceName(_)
                    )
                )));
            }
        }
    }
}
