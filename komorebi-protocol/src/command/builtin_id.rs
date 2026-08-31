use std::fmt;

use super::ActionId;

macro_rules! built_in_action_ids {
    ($($variant:ident => $id:literal),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
        pub enum BuiltInActionId {
            $($variant),+
        }

        impl BuiltInActionId {
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $id),+
                }
            }

            #[must_use]
            pub fn into_action_id(self) -> ActionId {
                ActionId::from_known(self.as_str())
            }
        }
    };
}

built_in_action_ids! {
    FocusWindow => "focus-window",
    MoveWindow => "move-window",
    ResizeWindow => "resize-window",
    ResizeWindowByStep => "resize-window-by-step",
    SetWorkspaceLayout => "set-workspace-layout",
    ToggleWindowFloat => "toggle-window-float",
    CycleFocusWindow => "cycle-focus-window",
    CycleMoveWindow => "cycle-move-window",
    ToggleWindowMonocle => "toggle-window-monocle",
    ToggleWindowMaximize => "toggle-window-maximize",
    ToggleContainerLock => "toggle-container-lock",
    StackWindow => "stack-window",
    UnstackWindow => "unstack-window",
    StackAll => "stack-all",
    UnstackAll => "unstack-all",
    CycleStack => "cycle-stack",
    CycleStackIndex => "cycle-stack-index",
    FocusStackWindow => "focus-stack-window",
    FocusWorkspace => "focus-workspace",
    CycleFocusWorkspace => "cycle-focus-workspace",
    CycleFocusEmptyWorkspace => "cycle-focus-empty-workspace",
    FocusLastWorkspace => "focus-last-workspace",
    CloseWorkspace => "close-workspace",
    FocusMonitor => "focus-monitor",
    CycleFocusMonitor => "cycle-focus-monitor",
    FocusMonitorAtCursor => "focus-monitor-at-cursor",
    FocusWorkspaceOnAllMonitors => "focus-workspace-on-all-monitors",
    FocusMonitorWorkspace => "focus-monitor-workspace",
    CloseWindow => "close-window",
    MinimizeWindow => "minimize-window",
    ForceFocus => "force-focus",
    PromoteContainer => "promote-container",
    PromoteContainerSwap => "promote-container-swap",
    PromoteFocus => "promote-focus",
    PromoteWindow => "promote-window",
    NewWorkspace => "new-workspace",
    ToggleTiling => "toggle-tiling",
    CycleLayout => "cycle-layout",
    FlipLayout => "flip-layout",
    ToggleWorkspaceLayer => "toggle-workspace-layer",
    MoveContainerToLastWorkspace => "move-container-to-last-workspace",
    SendContainerToLastWorkspace => "send-container-to-last-workspace",
    MoveContainerToWorkspace => "move-container-to-workspace",
    CycleMoveContainerToWorkspace => "cycle-move-container-to-workspace",
    SendContainerToWorkspace => "send-container-to-workspace",
    CycleSendContainerToWorkspace => "cycle-send-container-to-workspace",
    MoveContainerToMonitor => "move-container-to-monitor",
    CycleMoveContainerToMonitor => "cycle-move-container-to-monitor",
    SendContainerToMonitor => "send-container-to-monitor",
    CycleSendContainerToMonitor => "cycle-send-container-to-monitor",
    MoveContainerToMonitorWorkspace => "move-container-to-monitor-workspace",
    SendContainerToMonitorWorkspace => "send-container-to-monitor-workspace",
    MoveWorkspaceToMonitor => "move-workspace-to-monitor",
    CycleMoveWorkspaceToMonitor => "cycle-move-workspace-to-monitor",
    SwapWorkspacesToMonitor => "swap-workspaces-to-monitor",
    PreselectDirection => "preselect-direction",
    CancelPreselect => "cancel-preselect",
    Retile => "retile",
    RetileWithResizeDimensions => "retile-with-resize-dimensions",
    ManageFocusedWindow => "manage-focused-window",
    UnmanageFocusedWindow => "unmanage-focused-window",
    AdjustContainerPadding => "adjust-container-padding",
    AdjustWorkspacePadding => "adjust-workspace-padding",
    ToggleMouseFollowsFocus => "toggle-mouse-follows-focus",
    SetMouseFollowsFocus => "set-mouse-follows-focus",
    ToggleWindowContainerBehaviour => "toggle-window-container-behaviour",
    ToggleFloatOverride => "toggle-float-override",
    ToggleWorkspaceWindowContainerBehaviour => "toggle-workspace-window-container-behaviour",
    ToggleWorkspaceFloatOverride => "toggle-workspace-float-override",
    ToggleCrossMonitorMoveBehaviour => "toggle-cross-monitor-move-behaviour",
    ToggleMonocleFocusBehaviour => "toggle-monocle-focus-behaviour",
    TogglePause => "toggle-pause",
    SetFocusedContainerPadding => "set-focused-container-padding",
    SetFocusedWorkspacePadding => "set-focused-workspace-padding",
    SetContainerPadding => "set-container-padding",
    SetWorkspacePadding => "set-workspace-padding",
    SetWorkspaceTiling => "set-workspace-tiling",
    SetWorkspaceMonocle => "set-workspace-monocle",
    SetMonitorWorkspaceLayout => "set-monitor-workspace-layout",
    EnsureWorkspaces => "ensure-workspaces",
    ClearWorkspaceLayoutRules => "clear-workspace-layout-rules",
    SetScrollingColumns => "set-scrolling-columns",
    LockContainer => "lock-container",
    UnlockContainer => "unlock-container",
    ToggleTitleBars => "toggle-title-bars",
    EnforceWorkspaceRules => "enforce-workspace-rules",
    AddSessionFloatRule => "add-session-float-rule",
    ClearSessionFloatRules => "clear-session-float-rules",
    ResizeWindowEdge => "resize-window-edge",
    ResizeWindowEdgeByStep => "resize-window-edge-by-step",
    SetWindowHidingBehaviour => "set-window-hiding-behaviour",
    SetCrossMonitorMoveBehaviour => "set-cross-monitor-move-behaviour",
    SetMonocleFocusBehaviour => "set-monocle-focus-behaviour",
    SetUnmanagedWindowOperationBehaviour => "set-unmanaged-window-operation-behaviour",
    SetFocusFollowsMouse => "set-focus-follows-mouse",
    ToggleFocusFollowsMouse => "toggle-focus-follows-mouse",
    AddWorkspaceLayoutRule => "add-workspace-layout-rule",
    FocusNamedWorkspace => "focus-named-workspace",
    MoveContainerToNamedWorkspace => "move-container-to-named-workspace",
    SendContainerToNamedWorkspace => "send-container-to-named-workspace",
    SetNamedWorkspaceContainerPadding => "set-named-workspace-container-padding",
    SetNamedWorkspacePadding => "set-named-workspace-padding",
    SetNamedWorkspaceTiling => "set-named-workspace-tiling",
    SetNamedWorkspaceLayout => "set-named-workspace-layout",
    SetNamedWorkspaceCustomLayout => "set-named-workspace-custom-layout",
    AddNamedWorkspaceLayoutRule => "add-named-workspace-layout-rule",
    AddNamedWorkspaceCustomLayoutRule => "add-named-workspace-custom-layout-rule",
    ClearNamedWorkspaceLayoutRules => "clear-named-workspace-layout-rules",
    EnsureNamedWorkspaces => "ensure-named-workspaces",
    SetWorkspaceName => "set-workspace-name",
    SetLayoutRatios => "set-layout-ratios",
    SetCustomLayout => "set-custom-layout",
    SetWorkspaceCustomLayout => "set-workspace-custom-layout",
    AddWorkspaceCustomLayoutRule => "add-workspace-custom-layout-rule",
    EagerFocus => "eager-focus",
    RemoveTitleBar => "remove-title-bar",
    SetResizeStep => "set-resize-step",
    SetTransparencyEnabled => "set-transparency-enabled",
    ToggleTransparency => "toggle-transparency",
    SetTransparencyAlpha => "set-transparency-alpha",
    SetBorderEnabled => "set-border-enabled",
    SetBorderColour => "set-border-colour",
    SetBorderWidth => "set-border-width",
    SetBorderOffset => "set-border-offset",
    SetBorderStyle => "set-border-style",
    SetBorderImplementation => "set-border-implementation",
    SetStackbarMode => "set-stackbar-mode",
    SetStackbarLabel => "set-stackbar-label",
    SetStackbarFocusedTextColour => "set-stackbar-focused-text-colour",
    SetStackbarUnfocusedTextColour => "set-stackbar-unfocused-text-colour",
    SetStackbarBackgroundColour => "set-stackbar-background-colour",
    SetStackbarHeight => "set-stackbar-height",
    SetStackbarTabWidth => "set-stackbar-tab-width",
    SetStackbarFontSize => "set-stackbar-font-size",
    SetStackbarFontFamily => "set-stackbar-font-family",
    SetAnimationEnabled => "set-animation-enabled",
    SetAnimationDuration => "set-animation-duration",
    SetAnimationFps => "set-animation-fps",
    SetAnimationStyle => "set-animation-style",
    SetGlobalWorkAreaOffset => "set-global-work-area-offset",
    SetMonitorWorkAreaOffset => "set-monitor-work-area-offset",
    SetWorkspaceWorkAreaOffset => "set-workspace-work-area-offset",
    ToggleWindowBasedWorkAreaOffset => "toggle-window-based-work-area-offset",
}

impl fmt::Display for BuiltInActionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn every_builtin_identity_is_unique_and_valid() {
        let identities = BuiltInActionId::ALL
            .iter()
            .map(|action| action.as_str())
            .collect::<BTreeSet<_>>();
        assert_eq!(identities.len(), BuiltInActionId::ALL.len());
        for action in BuiltInActionId::ALL {
            assert_eq!(action.into_action_id().as_str(), action.as_str());
        }
    }
}
