use crate::action::BuiltinActionKind;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum FocusRequirement {
    None,
    FocusedWindow,
    DirectionalTarget,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DynamicChoiceSource {
    None,
    DirectionalTarget,
    ExistingWorkspaceName,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CurrentValueSource {
    None,
    Layout,
    Floating,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct OfferPolicy {
    pub focus: FocusRequirement,
    pub choices: DynamicChoiceSource,
    pub current_value: CurrentValueSource,
}

const AVAILABLE: OfferPolicy = OfferPolicy {
    focus: FocusRequirement::None,
    choices: DynamicChoiceSource::None,
    current_value: CurrentValueSource::None,
};

const FOCUSED: OfferPolicy = OfferPolicy {
    focus: FocusRequirement::FocusedWindow,
    choices: DynamicChoiceSource::None,
    current_value: CurrentValueSource::None,
};

const DIRECTIONAL: OfferPolicy = OfferPolicy {
    focus: FocusRequirement::DirectionalTarget,
    choices: DynamicChoiceSource::DirectionalTarget,
    current_value: CurrentValueSource::None,
};

const EXISTING_NAMED_WORKSPACE: OfferPolicy = OfferPolicy {
    focus: FocusRequirement::None,
    choices: DynamicChoiceSource::ExistingWorkspaceName,
    current_value: CurrentValueSource::None,
};

const FOCUSED_EXISTING_NAMED_WORKSPACE: OfferPolicy = OfferPolicy {
    focus: FocusRequirement::FocusedWindow,
    choices: DynamicChoiceSource::ExistingWorkspaceName,
    current_value: CurrentValueSource::None,
};

pub(super) const fn policy(kind: BuiltinActionKind) -> OfferPolicy {
    match kind {
        BuiltinActionKind::FocusWindow
        | BuiltinActionKind::MoveWindow
        | BuiltinActionKind::PromoteWindow => DIRECTIONAL,
        BuiltinActionKind::ToggleWindowFloat => OfferPolicy {
            current_value: CurrentValueSource::Floating,
            ..FOCUSED
        },
        BuiltinActionKind::SetWorkspaceLayout => OfferPolicy {
            current_value: CurrentValueSource::Layout,
            ..AVAILABLE
        },
        BuiltinActionKind::MoveContainerToNamedWorkspace
        | BuiltinActionKind::SendContainerToNamedWorkspace => FOCUSED_EXISTING_NAMED_WORKSPACE,
        BuiltinActionKind::FocusNamedWorkspace
        | BuiltinActionKind::SetNamedWorkspaceContainerPadding
        | BuiltinActionKind::SetNamedWorkspacePadding
        | BuiltinActionKind::SetNamedWorkspaceTiling
        | BuiltinActionKind::SetNamedWorkspaceLayout
        | BuiltinActionKind::SetNamedWorkspaceCustomLayout
        | BuiltinActionKind::AddNamedWorkspaceLayoutRule
        | BuiltinActionKind::AddNamedWorkspaceCustomLayoutRule
        | BuiltinActionKind::ClearNamedWorkspaceLayoutRules => EXISTING_NAMED_WORKSPACE,
        BuiltinActionKind::ResizeWindow
        | BuiltinActionKind::ResizeWindowByStep
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
        | BuiltinActionKind::ResizeWindowEdgeByStep
        | BuiltinActionKind::EagerFocus => FOCUSED,
        BuiltinActionKind::StackAll
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
        | BuiltinActionKind::SetResizeStep
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
        | BuiltinActionKind::EnsureNamedWorkspaces
        | BuiltinActionKind::SetWorkspaceName
        | BuiltinActionKind::SetLayoutRatios
        | BuiltinActionKind::SetCustomLayout
        | BuiltinActionKind::SetWorkspaceCustomLayout
        | BuiltinActionKind::AddWorkspaceCustomLayoutRule
        | BuiltinActionKind::RemoveTitleBar => AVAILABLE,
    }
}
