use crate::core::DefaultLayout;
use crate::core::OperationDirection;

use super::builtin::BuiltinActionKind;
use super::builtin::WorkspaceName;
use super::definition::ActionDefinition;
use super::definition::layout_name;
use super::id::ActionId;
use super::id::PrincipalId;
use super::id::Revision;
use super::id::WindowId;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActionSnapshot {
    pub revision: Revision,
    pub paused: bool,
    pub focused_window: Option<WindowId>,
    pub neighbor_left: bool,
    pub neighbor_right: bool,
    pub neighbor_up: bool,
    pub neighbor_down: bool,
    pub current_layout: DefaultLayout,
    pub focused_window_floating: bool,
    pub named_workspaces: Vec<(WorkspaceName, usize, usize)>,
    pub bindings: Vec<BindingHint>,
}

impl ActionSnapshot {
    #[must_use]
    pub fn empty() -> Self {
        Self {
            revision: Revision::new(0),
            paused: false,
            focused_window: None,
            neighbor_left: false,
            neighbor_right: false,
            neighbor_up: false,
            neighbor_down: false,
            current_layout: DefaultLayout::BSP,
            focused_window_floating: false,
            named_workspaces: Vec::new(),
            bindings: Vec::new(),
        }
    }

    #[must_use]
    pub fn workspace_by_name(&self, name: &WorkspaceName) -> Option<(usize, usize)> {
        self.named_workspaces
            .iter()
            .find(|(workspace_name, _, _)| workspace_name == name)
            .map(|(_, monitor, workspace)| (*monitor, *workspace))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BindingHint {
    pub action: ActionId,
    pub trigger: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActionGrants {
    kinds: Vec<BuiltinActionKind>,
}

impl ActionGrants {
    #[must_use]
    pub fn all() -> Self {
        Self {
            kinds: BuiltinActionKind::ALL.to_vec(),
        }
    }

    #[must_use]
    pub fn none() -> Self {
        Self { kinds: Vec::new() }
    }

    #[must_use]
    pub fn contains(&self, kind: BuiltinActionKind) -> bool {
        self.kinds.contains(&kind)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActionAuthority {
    pub principal: PrincipalId,
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
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActionOffer {
    pub definition: &'static ActionDefinition,
    pub revision: Revision,
    pub availability: ActionAvailability,
    pub current_value: Option<ActionCurrentValue>,
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
    let availability = availability(snapshot, authority, kind);
    let current_value = match kind {
        BuiltinActionKind::SetWorkspaceLayout => {
            Some(ActionCurrentValue::Layout(snapshot.current_layout))
        }
        BuiltinActionKind::ToggleWindowFloat => Some(ActionCurrentValue::Floating(
            snapshot.focused_window_floating,
        )),
        BuiltinActionKind::FocusWindow
        | BuiltinActionKind::MoveWindow
        | BuiltinActionKind::ResizeWindow
        | BuiltinActionKind::CycleFocusWindow
        | BuiltinActionKind::CycleMoveWindow
        | BuiltinActionKind::ToggleWindowMonocle
        | BuiltinActionKind::ToggleWindowMaximize
        | BuiltinActionKind::ToggleContainerLock
        | BuiltinActionKind::StackWindow
        | BuiltinActionKind::UnstackWindow
        | BuiltinActionKind::StackAll
        | BuiltinActionKind::UnstackAll
        | BuiltinActionKind::CycleStack
        | BuiltinActionKind::CycleStackIndex
        | BuiltinActionKind::FocusStackWindow
        | BuiltinActionKind::FocusWorkspace
        | BuiltinActionKind::CycleFocusWorkspace
        | BuiltinActionKind::CycleFocusEmptyWorkspace
        | BuiltinActionKind::FocusLastWorkspace
        | BuiltinActionKind::CloseWorkspace
        | BuiltinActionKind::FocusMonitor
        | BuiltinActionKind::CycleFocusMonitor
        | BuiltinActionKind::FocusMonitorAtCursor
        | BuiltinActionKind::FocusWorkspaceOnAllMonitors
        | BuiltinActionKind::FocusMonitorWorkspace
        | BuiltinActionKind::CloseWindow
        | BuiltinActionKind::MinimizeWindow
        | BuiltinActionKind::ForceFocus
        | BuiltinActionKind::PromoteContainer
        | BuiltinActionKind::PromoteContainerSwap
        | BuiltinActionKind::PromoteFocus
        | BuiltinActionKind::PromoteWindow
        | BuiltinActionKind::NewWorkspace
        | BuiltinActionKind::ToggleTiling
        | BuiltinActionKind::CycleLayout
        | BuiltinActionKind::FlipLayout
        | BuiltinActionKind::ToggleWorkspaceLayer
        | BuiltinActionKind::MoveContainerToLastWorkspace
        | BuiltinActionKind::SendContainerToLastWorkspace
        | BuiltinActionKind::MoveContainerToWorkspace
        | BuiltinActionKind::CycleMoveContainerToWorkspace
        | BuiltinActionKind::SendContainerToWorkspace
        | BuiltinActionKind::CycleSendContainerToWorkspace
        | BuiltinActionKind::MoveContainerToMonitor
        | BuiltinActionKind::CycleMoveContainerToMonitor
        | BuiltinActionKind::SendContainerToMonitor
        | BuiltinActionKind::CycleSendContainerToMonitor
        | BuiltinActionKind::MoveContainerToMonitorWorkspace
        | BuiltinActionKind::SendContainerToMonitorWorkspace
        | BuiltinActionKind::MoveWorkspaceToMonitor
        | BuiltinActionKind::CycleMoveWorkspaceToMonitor
        | BuiltinActionKind::SwapWorkspacesToMonitor
        | BuiltinActionKind::PreselectDirection
        | BuiltinActionKind::CancelPreselect
        | BuiltinActionKind::Retile
        | BuiltinActionKind::RetileWithResizeDimensions
        | BuiltinActionKind::ManageFocusedWindow
        | BuiltinActionKind::UnmanageFocusedWindow
        | BuiltinActionKind::AdjustContainerPadding
        | BuiltinActionKind::AdjustWorkspacePadding
        | BuiltinActionKind::ToggleMouseFollowsFocus
        | BuiltinActionKind::SetMouseFollowsFocus
        | BuiltinActionKind::ToggleWindowContainerBehaviour
        | BuiltinActionKind::ToggleFloatOverride
        | BuiltinActionKind::ToggleWorkspaceWindowContainerBehaviour
        | BuiltinActionKind::ToggleWorkspaceFloatOverride
        | BuiltinActionKind::ToggleCrossMonitorMoveBehaviour
        | BuiltinActionKind::ToggleMonocleFocusBehaviour
        | BuiltinActionKind::TogglePause
        | BuiltinActionKind::SetFocusedContainerPadding
        | BuiltinActionKind::SetFocusedWorkspacePadding
        | BuiltinActionKind::SetContainerPadding
        | BuiltinActionKind::SetWorkspacePadding
        | BuiltinActionKind::SetWorkspaceTiling
        | BuiltinActionKind::SetMonitorWorkspaceLayout
        | BuiltinActionKind::EnsureWorkspaces
        | BuiltinActionKind::ClearWorkspaceLayoutRules
        | BuiltinActionKind::SetScrollingColumns
        | BuiltinActionKind::LockContainer
        | BuiltinActionKind::UnlockContainer
        | BuiltinActionKind::ToggleTitleBars
        | BuiltinActionKind::EnforceWorkspaceRules
        | BuiltinActionKind::AddSessionFloatRule
        | BuiltinActionKind::ClearSessionFloatRules
        | BuiltinActionKind::ResizeWindowEdge
        | BuiltinActionKind::SetWindowHidingBehaviour
        | BuiltinActionKind::SetCrossMonitorMoveBehaviour
        | BuiltinActionKind::SetMonocleFocusBehaviour
        | BuiltinActionKind::SetUnmanagedWindowOperationBehaviour
        | BuiltinActionKind::SetFocusFollowsMouse
        | BuiltinActionKind::ToggleFocusFollowsMouse
        | BuiltinActionKind::AddWorkspaceLayoutRule
        | BuiltinActionKind::FocusNamedWorkspace
        | BuiltinActionKind::MoveContainerToNamedWorkspace
        | BuiltinActionKind::SendContainerToNamedWorkspace
        | BuiltinActionKind::SetNamedWorkspaceContainerPadding
        | BuiltinActionKind::SetNamedWorkspacePadding
        | BuiltinActionKind::SetNamedWorkspaceTiling
        | BuiltinActionKind::SetNamedWorkspaceLayout
        | BuiltinActionKind::SetNamedWorkspaceCustomLayout
        | BuiltinActionKind::AddNamedWorkspaceLayoutRule
        | BuiltinActionKind::AddNamedWorkspaceCustomLayoutRule
        | BuiltinActionKind::ClearNamedWorkspaceLayoutRules
        | BuiltinActionKind::EnsureNamedWorkspaces
        | BuiltinActionKind::SetWorkspaceName
        | BuiltinActionKind::SetLayoutRatios
        | BuiltinActionKind::SetCustomLayout
        | BuiltinActionKind::SetWorkspaceCustomLayout
        | BuiltinActionKind::AddWorkspaceCustomLayoutRule
        | BuiltinActionKind::EagerFocus
        | BuiltinActionKind::RemoveTitleBar => None,
    };
    let bindings = snapshot
        .bindings
        .iter()
        .filter(|hint| hint.action == definition.id)
        .cloned()
        .collect();
    ActionOffer {
        definition,
        revision: snapshot.revision,
        availability,
        current_value,
        bindings,
    }
}

fn availability(
    snapshot: &ActionSnapshot,
    authority: &ActionAuthority,
    kind: BuiltinActionKind,
) -> ActionAvailability {
    if !authority.grants.contains(kind) {
        return ActionAvailability::Unavailable(Unavailability::Unauthorized);
    }
    if kind == BuiltinActionKind::TogglePause {
        return ActionAvailability::Available;
    }
    if snapshot.paused {
        return ActionAvailability::Unavailable(Unavailability::ManagerPaused);
    }
    match kind {
        BuiltinActionKind::FocusWindow | BuiltinActionKind::MoveWindow => {
            if snapshot.focused_window.is_none() {
                ActionAvailability::Unavailable(Unavailability::NoFocusedWindow)
            } else if snapshot.neighbor_left
                || snapshot.neighbor_right
                || snapshot.neighbor_up
                || snapshot.neighbor_down
            {
                ActionAvailability::Available
            } else {
                ActionAvailability::Unavailable(Unavailability::NoWindowInDirection)
            }
        }
        BuiltinActionKind::ResizeWindow
        | BuiltinActionKind::ToggleWindowFloat
        | BuiltinActionKind::CycleFocusWindow
        | BuiltinActionKind::CycleMoveWindow
        | BuiltinActionKind::ToggleWindowMonocle
        | BuiltinActionKind::ToggleWindowMaximize
        | BuiltinActionKind::ToggleContainerLock
        | BuiltinActionKind::StackWindow
        | BuiltinActionKind::UnstackWindow
        | BuiltinActionKind::CycleStack
        | BuiltinActionKind::CycleStackIndex
        | BuiltinActionKind::FocusStackWindow
        | BuiltinActionKind::CloseWindow
        | BuiltinActionKind::MinimizeWindow
        | BuiltinActionKind::ForceFocus
        | BuiltinActionKind::PromoteContainer
        | BuiltinActionKind::PromoteContainerSwap
        | BuiltinActionKind::PromoteFocus
        | BuiltinActionKind::PromoteWindow
        | BuiltinActionKind::MoveContainerToLastWorkspace
        | BuiltinActionKind::SendContainerToLastWorkspace
        | BuiltinActionKind::MoveContainerToWorkspace
        | BuiltinActionKind::CycleMoveContainerToWorkspace
        | BuiltinActionKind::SendContainerToWorkspace
        | BuiltinActionKind::CycleSendContainerToWorkspace
        | BuiltinActionKind::MoveContainerToMonitor
        | BuiltinActionKind::CycleMoveContainerToMonitor
        | BuiltinActionKind::SendContainerToMonitor
        | BuiltinActionKind::CycleSendContainerToMonitor
        | BuiltinActionKind::MoveContainerToMonitorWorkspace
        | BuiltinActionKind::SendContainerToMonitorWorkspace
        | BuiltinActionKind::PreselectDirection
        | BuiltinActionKind::UnmanageFocusedWindow
        | BuiltinActionKind::AddSessionFloatRule
        | BuiltinActionKind::ResizeWindowEdge
        | BuiltinActionKind::MoveContainerToNamedWorkspace
        | BuiltinActionKind::SendContainerToNamedWorkspace
        | BuiltinActionKind::EagerFocus => {
            if snapshot.focused_window.is_some() {
                ActionAvailability::Available
            } else {
                ActionAvailability::Unavailable(Unavailability::NoFocusedWindow)
            }
        }
        BuiltinActionKind::SetWorkspaceLayout
        | BuiltinActionKind::StackAll
        | BuiltinActionKind::UnstackAll
        | BuiltinActionKind::FocusWorkspace
        | BuiltinActionKind::CycleFocusWorkspace
        | BuiltinActionKind::CycleFocusEmptyWorkspace
        | BuiltinActionKind::FocusLastWorkspace
        | BuiltinActionKind::CloseWorkspace
        | BuiltinActionKind::FocusMonitor
        | BuiltinActionKind::CycleFocusMonitor
        | BuiltinActionKind::FocusMonitorAtCursor
        | BuiltinActionKind::FocusWorkspaceOnAllMonitors
        | BuiltinActionKind::FocusMonitorWorkspace
        | BuiltinActionKind::NewWorkspace
        | BuiltinActionKind::ToggleTiling
        | BuiltinActionKind::CycleLayout
        | BuiltinActionKind::FlipLayout
        | BuiltinActionKind::ToggleWorkspaceLayer
        | BuiltinActionKind::MoveWorkspaceToMonitor
        | BuiltinActionKind::CycleMoveWorkspaceToMonitor
        | BuiltinActionKind::SwapWorkspacesToMonitor
        | BuiltinActionKind::CancelPreselect
        | BuiltinActionKind::Retile
        | BuiltinActionKind::RetileWithResizeDimensions
        | BuiltinActionKind::ManageFocusedWindow
        | BuiltinActionKind::AdjustContainerPadding
        | BuiltinActionKind::AdjustWorkspacePadding
        | BuiltinActionKind::ToggleMouseFollowsFocus
        | BuiltinActionKind::SetMouseFollowsFocus
        | BuiltinActionKind::ToggleWindowContainerBehaviour
        | BuiltinActionKind::ToggleFloatOverride
        | BuiltinActionKind::ToggleWorkspaceWindowContainerBehaviour
        | BuiltinActionKind::ToggleWorkspaceFloatOverride
        | BuiltinActionKind::ToggleCrossMonitorMoveBehaviour
        | BuiltinActionKind::ToggleMonocleFocusBehaviour
        | BuiltinActionKind::TogglePause
        | BuiltinActionKind::SetFocusedContainerPadding
        | BuiltinActionKind::SetFocusedWorkspacePadding
        | BuiltinActionKind::SetContainerPadding
        | BuiltinActionKind::SetWorkspacePadding
        | BuiltinActionKind::SetWorkspaceTiling
        | BuiltinActionKind::SetMonitorWorkspaceLayout
        | BuiltinActionKind::EnsureWorkspaces
        | BuiltinActionKind::ClearWorkspaceLayoutRules
        | BuiltinActionKind::SetScrollingColumns
        | BuiltinActionKind::LockContainer
        | BuiltinActionKind::UnlockContainer
        | BuiltinActionKind::ToggleTitleBars
        | BuiltinActionKind::EnforceWorkspaceRules
        | BuiltinActionKind::ClearSessionFloatRules
        | BuiltinActionKind::SetWindowHidingBehaviour
        | BuiltinActionKind::SetCrossMonitorMoveBehaviour
        | BuiltinActionKind::SetMonocleFocusBehaviour
        | BuiltinActionKind::SetUnmanagedWindowOperationBehaviour
        | BuiltinActionKind::SetFocusFollowsMouse
        | BuiltinActionKind::ToggleFocusFollowsMouse
        | BuiltinActionKind::AddWorkspaceLayoutRule
        | BuiltinActionKind::FocusNamedWorkspace
        | BuiltinActionKind::SetNamedWorkspaceContainerPadding
        | BuiltinActionKind::SetNamedWorkspacePadding
        | BuiltinActionKind::SetNamedWorkspaceTiling
        | BuiltinActionKind::SetNamedWorkspaceLayout
        | BuiltinActionKind::SetNamedWorkspaceCustomLayout
        | BuiltinActionKind::AddNamedWorkspaceLayoutRule
        | BuiltinActionKind::AddNamedWorkspaceCustomLayoutRule
        | BuiltinActionKind::ClearNamedWorkspaceLayoutRules
        | BuiltinActionKind::EnsureNamedWorkspaces
        | BuiltinActionKind::SetWorkspaceName
        | BuiltinActionKind::SetLayoutRatios
        | BuiltinActionKind::SetCustomLayout
        | BuiltinActionKind::SetWorkspaceCustomLayout
        | BuiltinActionKind::AddWorkspaceCustomLayoutRule
        | BuiltinActionKind::RemoveTitleBar => ActionAvailability::Available,
    }
}

#[must_use]
pub fn neighbor_in(snapshot: &ActionSnapshot, direction: OperationDirection) -> bool {
    match direction {
        OperationDirection::Left => snapshot.neighbor_left,
        OperationDirection::Right => snapshot.neighbor_right,
        OperationDirection::Up => snapshot.neighbor_up,
        OperationDirection::Down => snapshot.neighbor_down,
    }
}

#[must_use]
pub fn current_layout_name(snapshot: &ActionSnapshot) -> &'static str {
    layout_name(snapshot.current_layout)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::id::Revision;

    fn live_snapshot() -> ActionSnapshot {
        ActionSnapshot {
            revision: Revision::new(3),
            paused: false,
            focused_window: Some(WindowId::new(1)),
            neighbor_left: true,
            neighbor_right: false,
            neighbor_up: false,
            neighbor_down: false,
            current_layout: DefaultLayout::BSP,
            focused_window_floating: false,
            named_workspaces: Vec::new(),
            bindings: vec![BindingHint {
                action: ActionId::FOCUS_WINDOW,
                trigger: "alt+h".to_owned(),
            }],
        }
    }

    #[test]
    fn paused_manager_keeps_actions_discoverable_with_a_reason() {
        let mut snapshot = live_snapshot();
        snapshot.paused = true;
        let authority = ActionAuthority {
            principal: PrincipalId::new(1),
            grants: ActionGrants::all(),
        };
        let catalog = offers(&snapshot, &authority);
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
        let authority = ActionAuthority {
            principal: PrincipalId::new(1),
            grants: ActionGrants::all(),
        };
        let focus = offers(&snapshot, &authority)
            .into_iter()
            .find(|offer| offer.definition.kind == BuiltinActionKind::FocusWindow)
            .expect("focus-window stays discoverable");
        assert_eq!(
            focus.availability,
            ActionAvailability::Unavailable(Unavailability::NoFocusedWindow)
        );
    }

    #[test]
    fn layout_offer_projects_the_current_built_in_layout() {
        let snapshot = live_snapshot();
        let authority = ActionAuthority {
            principal: PrincipalId::new(1),
            grants: ActionGrants::all(),
        };
        let layout = offers(&snapshot, &authority)
            .into_iter()
            .find(|offer| offer.definition.kind == BuiltinActionKind::SetWorkspaceLayout)
            .expect("layout offer");
        assert_eq!(
            layout.current_value,
            Some(ActionCurrentValue::Layout(DefaultLayout::BSP))
        );
        assert_eq!(current_layout_name(&snapshot), "bsp");
        assert_eq!(layout.revision, Revision::new(3));
    }
}
