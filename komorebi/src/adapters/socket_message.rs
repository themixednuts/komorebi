use crate::action::BuiltinAction;
use crate::action::WindowSelector;
use crate::action::WorkspaceSelector;
use crate::core::SocketMessage;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SocketMessageClass {
    Action,
    Query,
    Subscription,
    Configuration,
    SchemaDebugAdmin,
    InternalOnly,
}

#[must_use]
pub fn classify(message: &SocketMessage) -> SocketMessageClass {
    use SocketMessage::*;

    match message {
        FocusWindow(_)
        | MoveWindow(_)
        | PreselectDirection(_)
        | CancelPreselect
        | CycleFocusWindow(_)
        | CycleMoveWindow(_)
        | StackWindow(_)
        | UnstackWindow
        | CycleStack(_)
        | CycleStackIndex(_)
        | FocusStackWindow(_)
        | StackAll
        | UnstackAll
        | ResizeWindowEdge(_, _)
        | ResizeWindowAxis(_, _)
        | MoveContainerToLastWorkspace
        | SendContainerToLastWorkspace
        | MoveContainerToMonitorNumber(_)
        | CycleMoveContainerToMonitor(_)
        | MoveContainerToWorkspaceNumber(_)
        | MoveContainerToNamedWorkspace(_)
        | CycleMoveContainerToWorkspace(_)
        | SendContainerToMonitorNumber(_)
        | CycleSendContainerToMonitor(_)
        | SendContainerToWorkspaceNumber(_)
        | CycleSendContainerToWorkspace(_)
        | SendContainerToMonitorWorkspaceNumber(_, _)
        | MoveContainerToMonitorWorkspaceNumber(_, _)
        | SendContainerToNamedWorkspace(_)
        | CycleMoveWorkspaceToMonitor(_)
        | MoveWorkspaceToMonitorNumber(_)
        | SwapWorkspacesToMonitorNumber(_)
        | ForceFocus
        | Close
        | Minimize
        | Promote
        | PromoteSwap
        | PromoteFocus
        | PromoteWindow(_)
        | EagerFocus(_)
        | LockMonitorWorkspaceContainer(_, _, _)
        | UnlockMonitorWorkspaceContainer(_, _, _)
        | ToggleLock
        | ToggleFloat
        | ToggleMonocle
        | ToggleMaximize
        | ToggleWindowContainerBehaviour
        | ToggleFloatOverride
        | WindowHidingBehaviour(_)
        | ToggleCrossMonitorMoveBehaviour
        | CrossMonitorMoveBehaviour(_)
        | ToggleMonocleFocusBehaviour
        | MonocleFocusBehaviour(_)
        | UnmanagedWindowOperationBehaviour(_)
        | ManageFocusedWindow
        | UnmanageFocusedWindow
        | AdjustContainerPadding(_, _)
        | AdjustWorkspacePadding(_, _)
        | ChangeLayout(_)
        | CycleLayout(_)
        | LayoutRatios(_, _)
        | ScrollingLayoutColumns(_)
        | ChangeLayoutCustom(_)
        | FlipLayout(_)
        | ToggleWorkspaceWindowContainerBehaviour
        | ToggleWorkspaceFloatOverride
        | EnsureWorkspaces(_, _)
        | EnsureNamedWorkspaces(_, _)
        | NewWorkspace
        | ToggleTiling
        | TogglePause
        | Retile
        | RetileWithResizeDimensions
        | CycleFocusMonitor(_)
        | CycleFocusWorkspace(_)
        | CycleFocusEmptyWorkspace(_)
        | FocusMonitorNumber(_)
        | FocusMonitorAtCursor
        | FocusLastWorkspace
        | CloseWorkspace
        | FocusWorkspaceNumber(_)
        | FocusWorkspaceNumbers(_)
        | FocusMonitorWorkspaceNumber(_, _)
        | FocusNamedWorkspace(_)
        | ContainerPadding(_, _, _)
        | NamedWorkspaceContainerPadding(_, _)
        | FocusedWorkspaceContainerPadding(_)
        | WorkspacePadding(_, _, _)
        | NamedWorkspacePadding(_, _)
        | FocusedWorkspacePadding(_)
        | WorkspaceTiling(_, _, _)
        | NamedWorkspaceTiling(_, _)
        | WorkspaceName(_, _, _)
        | WorkspaceLayout(_, _, _)
        | NamedWorkspaceLayout(_, _)
        | WorkspaceLayoutCustom(_, _, _)
        | NamedWorkspaceLayoutCustom(_, _)
        | WorkspaceLayoutRule(_, _, _, _)
        | NamedWorkspaceLayoutRule(_, _, _)
        | WorkspaceLayoutCustomRule(_, _, _, _)
        | NamedWorkspaceLayoutCustomRule(_, _, _)
        | ClearWorkspaceLayoutRules(_, _)
        | ClearNamedWorkspaceLayoutRules(_)
        | ToggleWorkspaceLayer
        | FocusFollowsMouse(_, _)
        | ToggleFocusFollowsMouse(_)
        | MouseFollowsFocus(_)
        | ToggleMouseFollowsFocus
        | RemoveTitleBar(_, _)
        | ToggleTitleBars
        | SessionFloatRule
        | ClearSessionFloatRules
        | EnforceWorkspaceRules => SocketMessageClass::Action,

        State | GlobalState | VisibleWindows | MonitorInformation | Query(_)
        | SessionFloatRules => SocketMessageClass::Query,

        AddSubscriberSocket(_)
        | AddSubscriberSocketWithOptions(_, _)
        | RemoveSubscriberSocket(_)
        | AddSubscriberPipe(_)
        | RemoveSubscriberPipe(_) => SocketMessageClass::Subscription,

        MonitorIndexPreference(_, _, _, _, _)
        | DisplayIndexPreference(_, _)
        | ReloadConfiguration
        | ReplaceConfiguration(_)
        | ReloadStaticConfiguration(_)
        | WatchConfiguration(_)
        | CompleteConfiguration
        | AltFocusHack(_)
        | Theme(_)
        | Animation(_, _)
        | AnimationDuration(_, _)
        | AnimationFps(_)
        | AnimationStyle(_, _)
        | Border(_)
        | BorderColour(_, _, _, _)
        | BorderStyle(_)
        | BorderWidth(_)
        | BorderOffset(_)
        | BorderImplementation(_)
        | Transparency(_)
        | ToggleTransparency
        | TransparencyAlpha(_)
        | InvisibleBorders(_)
        | StackbarMode(_)
        | StackbarLabel(_)
        | StackbarFocusedTextColour(_, _, _)
        | StackbarUnfocusedTextColour(_, _, _)
        | StackbarBackgroundColour(_, _, _)
        | StackbarHeight(_)
        | StackbarTabWidth(_)
        | StackbarFontSize(_)
        | StackbarFontFamily(_)
        | WorkAreaOffset(_)
        | MonitorWorkAreaOffset(_, _)
        | WorkspaceWorkAreaOffset(_, _, _)
        | ToggleWindowBasedWorkAreaOffset
        | ResizeDelta(_)
        | InitialWorkspaceRule(_, _, _, _)
        | InitialNamedWorkspaceRule(_, _, _)
        | WorkspaceRule(_, _, _, _)
        | NamedWorkspaceRule(_, _, _)
        | ClearWorkspaceRules(_, _)
        | ClearNamedWorkspaceRules(_)
        | ClearAllWorkspaceRules
        | IgnoreRule(_, _)
        | ManageRule(_, _)
        | IdentifyObjectNameChangeApplication(_, _)
        | IdentifyTrayApplication(_, _)
        | IdentifyLayeredApplication(_, _)
        | IdentifyBorderOverflowApplication(_, _)
        | QuickSave
        | QuickLoad
        | Save(_)
        | Load(_) => SocketMessageClass::Configuration,

        Stop
        | StopIgnoreRestore
        | ApplicationSpecificConfigurationSchema
        | NotificationSchema
        | SocketSchema
        | StaticConfigSchema
        | GenerateStaticConfig
        | DebugWindow(_) => SocketMessageClass::SchemaDebugAdmin,

        ApplyState(_) => SocketMessageClass::InternalOnly,
    }
}

#[must_use]
pub fn to_builtin_action(message: &SocketMessage) -> Option<BuiltinAction> {
    match message {
        SocketMessage::FocusWindow(direction) => Some(BuiltinAction::FocusWindow {
            direction: *direction,
        }),
        SocketMessage::MoveWindow(direction) => Some(BuiltinAction::MoveWindow {
            direction: *direction,
        }),
        SocketMessage::ChangeLayout(layout) => Some(BuiltinAction::SetWorkspaceLayout {
            workspace: WorkspaceSelector::FocusedAtExecution,
            layout: *layout,
        }),
        SocketMessage::ToggleFloat => Some(BuiltinAction::ToggleWindowFloat {
            window: WindowSelector::FocusedAtExecution,
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::DefaultLayout;
    use crate::core::OperationDirection;

    #[test]
    fn migrated_socket_messages_become_the_same_builtin_action() {
        assert_eq!(
            to_builtin_action(&SocketMessage::FocusWindow(OperationDirection::Left)),
            Some(BuiltinAction::FocusWindow {
                direction: OperationDirection::Left,
            })
        );
        assert_eq!(
            classify(&SocketMessage::FocusWindow(OperationDirection::Left)),
            SocketMessageClass::Action
        );
        assert_eq!(
            to_builtin_action(&SocketMessage::ChangeLayout(DefaultLayout::Columns)),
            Some(BuiltinAction::SetWorkspaceLayout {
                workspace: WorkspaceSelector::FocusedAtExecution,
                layout: DefaultLayout::Columns,
            })
        );
        assert_eq!(classify(&SocketMessage::State), SocketMessageClass::Query);
        assert_eq!(
            classify(&SocketMessage::AddSubscriberSocket(
                crate::core::SubscriberName::parse("komorebi-bar-forest").unwrap()
            )),
            SocketMessageClass::Subscription
        );
        assert_eq!(
            classify(&SocketMessage::Stop),
            SocketMessageClass::SchemaDebugAdmin
        );
    }

    #[test]
    fn socket_focus_admits_as_the_same_catalog_action() {
        use crate::action::ActionGrants;
        use crate::action::ActionSnapshot;
        use crate::action::CatalogState;
        use crate::action::InvocationContext;
        use crate::action::InvocationId;
        use crate::action::InvocationOrigin;
        use crate::action::InvokeAction;
        use crate::action::PrincipalId;
        use crate::action::Revision;
        use crate::action::id::WindowId;
        use crate::action::invoke::ActionAdmission;
        use std::time::Instant;

        let message = SocketMessage::FocusWindow(OperationDirection::Left);
        let action = to_builtin_action(&message).expect("focus-window is migrated");
        let mut state = CatalogState::new(ActionSnapshot {
            revision: Revision::new(1),
            paused: false,
            focused_window: Some(WindowId::new(9)),
            neighbor_left: true,
            neighbor_right: false,
            neighbor_up: false,
            neighbor_down: false,
            current_layout: DefaultLayout::BSP,
            focused_window_floating: false,
            bindings: Vec::new(),
        });
        let admission = state.admit(
            InvokeAction {
                invocation_id: InvocationId::new(),
                expected_revision: Revision::new(1),
                action,
                confirmation: None,
            },
            &InvocationContext {
                principal: PrincipalId::new(1),
                origin: InvocationOrigin::Cli,
                grants: ActionGrants::all(),
            },
            Instant::now(),
        );
        assert!(matches!(
            admission,
            ActionAdmission::Committed { revision, .. } if revision == Revision::new(2)
        ));
    }
}
