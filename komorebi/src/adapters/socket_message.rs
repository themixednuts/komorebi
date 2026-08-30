use crate::action::BuiltinAction;
use crate::action::ContainerIndex;
use crate::action::MonitorIndex;
use crate::action::Pixels;
use crate::action::StackIndex;
use crate::action::WindowSelector;
use crate::action::WindowsPath;
use crate::action::WorkspaceIndex;
use crate::action::WorkspaceName;
use crate::action::WorkspaceSelector;
use crate::core::Sizing;
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum SocketActionAdapterError {
    #[error("socket message classified as an action has invalid parameters")]
    InvalidParameters,
    #[error("socket message classified as {classification:?} produced a built-in action")]
    ClassificationMismatch { classification: SocketMessageClass },
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
pub fn to_builtin_action(message: &SocketMessage, resize_delta: i32) -> Option<BuiltinAction> {
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
        SocketMessage::ResizeWindowAxis(axis, sizing) => {
            let signed = match sizing {
                Sizing::Increase => resize_delta,
                Sizing::Decrease => -resize_delta,
            };
            Some(BuiltinAction::ResizeWindow {
                axis: *axis,
                delta: Pixels::new(signed).ok()?,
            })
        }
        SocketMessage::CycleFocusWindow(direction) => Some(BuiltinAction::CycleFocusWindow {
            direction: *direction,
        }),
        SocketMessage::CycleMoveWindow(direction) => Some(BuiltinAction::CycleMoveWindow {
            direction: *direction,
        }),
        SocketMessage::ToggleMonocle => Some(BuiltinAction::ToggleWindowMonocle {
            window: WindowSelector::FocusedAtExecution,
        }),
        SocketMessage::ToggleMaximize => Some(BuiltinAction::ToggleWindowMaximize {
            window: WindowSelector::FocusedAtExecution,
        }),
        SocketMessage::ToggleLock => Some(BuiltinAction::ToggleContainerLock {
            window: WindowSelector::FocusedAtExecution,
        }),
        SocketMessage::StackWindow(direction) => Some(BuiltinAction::StackWindow {
            direction: *direction,
        }),
        SocketMessage::UnstackWindow => Some(BuiltinAction::UnstackWindow {
            window: WindowSelector::FocusedAtExecution,
        }),
        SocketMessage::StackAll => Some(BuiltinAction::StackAll),
        SocketMessage::UnstackAll => Some(BuiltinAction::UnstackAll),
        SocketMessage::CycleStack(direction) => Some(BuiltinAction::CycleStack {
            direction: *direction,
        }),
        SocketMessage::CycleStackIndex(direction) => Some(BuiltinAction::CycleStackIndex {
            direction: *direction,
        }),
        SocketMessage::FocusStackWindow(index) => Some(BuiltinAction::FocusStackWindow {
            index: StackIndex::new(*index),
        }),
        SocketMessage::FocusWorkspaceNumber(index) => Some(BuiltinAction::FocusWorkspace {
            index: WorkspaceIndex::new(*index),
        }),
        SocketMessage::CycleFocusWorkspace(direction) => Some(BuiltinAction::CycleFocusWorkspace {
            direction: *direction,
        }),
        SocketMessage::CycleFocusEmptyWorkspace(direction) => {
            Some(BuiltinAction::CycleFocusEmptyWorkspace {
                direction: *direction,
            })
        }
        SocketMessage::FocusLastWorkspace => Some(BuiltinAction::FocusLastWorkspace),
        SocketMessage::CloseWorkspace => Some(BuiltinAction::CloseWorkspace),
        SocketMessage::FocusMonitorNumber(index) => Some(BuiltinAction::FocusMonitor {
            index: MonitorIndex::new(*index),
        }),
        SocketMessage::CycleFocusMonitor(direction) => Some(BuiltinAction::CycleFocusMonitor {
            direction: *direction,
        }),
        SocketMessage::FocusMonitorAtCursor => Some(BuiltinAction::FocusMonitorAtCursor),
        SocketMessage::FocusWorkspaceNumbers(index) => {
            Some(BuiltinAction::FocusWorkspaceOnAllMonitors {
                index: WorkspaceIndex::new(*index),
            })
        }
        SocketMessage::FocusMonitorWorkspaceNumber(monitor, workspace) => {
            Some(BuiltinAction::FocusMonitorWorkspace {
                monitor: MonitorIndex::new(*monitor),
                workspace: WorkspaceIndex::new(*workspace),
            })
        }
        SocketMessage::Close => Some(BuiltinAction::CloseWindow {
            window: WindowSelector::FocusedAtExecution,
        }),
        SocketMessage::Minimize => Some(BuiltinAction::MinimizeWindow {
            window: WindowSelector::FocusedAtExecution,
        }),
        SocketMessage::ForceFocus => Some(BuiltinAction::ForceFocus {
            window: WindowSelector::FocusedAtExecution,
        }),
        SocketMessage::Promote => Some(BuiltinAction::PromoteContainer),
        SocketMessage::PromoteSwap => Some(BuiltinAction::PromoteContainerSwap),
        SocketMessage::PromoteFocus => Some(BuiltinAction::PromoteFocus),
        SocketMessage::PromoteWindow(direction) => Some(BuiltinAction::PromoteWindow {
            direction: *direction,
        }),
        SocketMessage::NewWorkspace => Some(BuiltinAction::NewWorkspace),
        SocketMessage::ToggleTiling => Some(BuiltinAction::ToggleTiling),
        SocketMessage::CycleLayout(direction) => Some(BuiltinAction::CycleLayout {
            direction: *direction,
        }),
        SocketMessage::FlipLayout(axis) => Some(BuiltinAction::FlipLayout { axis: *axis }),
        SocketMessage::ToggleWorkspaceLayer => Some(BuiltinAction::ToggleWorkspaceLayer),
        SocketMessage::MoveContainerToLastWorkspace => {
            Some(BuiltinAction::MoveContainerToLastWorkspace)
        }
        SocketMessage::SendContainerToLastWorkspace => {
            Some(BuiltinAction::SendContainerToLastWorkspace)
        }
        SocketMessage::MoveContainerToWorkspaceNumber(index) => {
            Some(BuiltinAction::MoveContainerToWorkspace {
                index: WorkspaceIndex::new(*index),
            })
        }
        SocketMessage::CycleMoveContainerToWorkspace(direction) => {
            Some(BuiltinAction::CycleMoveContainerToWorkspace {
                direction: *direction,
            })
        }
        SocketMessage::SendContainerToWorkspaceNumber(index) => {
            Some(BuiltinAction::SendContainerToWorkspace {
                index: WorkspaceIndex::new(*index),
            })
        }
        SocketMessage::CycleSendContainerToWorkspace(direction) => {
            Some(BuiltinAction::CycleSendContainerToWorkspace {
                direction: *direction,
            })
        }
        SocketMessage::MoveContainerToMonitorNumber(index) => {
            Some(BuiltinAction::MoveContainerToMonitor {
                index: MonitorIndex::new(*index),
            })
        }
        SocketMessage::CycleMoveContainerToMonitor(direction) => {
            Some(BuiltinAction::CycleMoveContainerToMonitor {
                direction: *direction,
            })
        }
        SocketMessage::SendContainerToMonitorNumber(index) => {
            Some(BuiltinAction::SendContainerToMonitor {
                index: MonitorIndex::new(*index),
            })
        }
        SocketMessage::CycleSendContainerToMonitor(direction) => {
            Some(BuiltinAction::CycleSendContainerToMonitor {
                direction: *direction,
            })
        }
        SocketMessage::MoveContainerToMonitorWorkspaceNumber(monitor, workspace) => {
            Some(BuiltinAction::MoveContainerToMonitorWorkspace {
                monitor: MonitorIndex::new(*monitor),
                workspace: WorkspaceIndex::new(*workspace),
            })
        }
        SocketMessage::SendContainerToMonitorWorkspaceNumber(monitor, workspace) => {
            Some(BuiltinAction::SendContainerToMonitorWorkspace {
                monitor: MonitorIndex::new(*monitor),
                workspace: WorkspaceIndex::new(*workspace),
            })
        }
        SocketMessage::MoveWorkspaceToMonitorNumber(index) => {
            Some(BuiltinAction::MoveWorkspaceToMonitor {
                index: MonitorIndex::new(*index),
            })
        }
        SocketMessage::CycleMoveWorkspaceToMonitor(direction) => {
            Some(BuiltinAction::CycleMoveWorkspaceToMonitor {
                direction: *direction,
            })
        }
        SocketMessage::SwapWorkspacesToMonitorNumber(index) => {
            Some(BuiltinAction::SwapWorkspacesToMonitor {
                index: MonitorIndex::new(*index),
            })
        }
        SocketMessage::PreselectDirection(direction) => Some(BuiltinAction::PreselectDirection {
            direction: *direction,
        }),
        SocketMessage::CancelPreselect => Some(BuiltinAction::CancelPreselect),
        SocketMessage::Retile => Some(BuiltinAction::Retile),
        SocketMessage::RetileWithResizeDimensions => {
            Some(BuiltinAction::RetileWithResizeDimensions)
        }
        SocketMessage::ManageFocusedWindow => Some(BuiltinAction::ManageFocusedWindow),
        SocketMessage::UnmanageFocusedWindow => Some(BuiltinAction::UnmanageFocusedWindow),
        SocketMessage::AdjustContainerPadding(sizing, adjustment) => {
            Some(BuiltinAction::AdjustContainerPadding {
                sizing: *sizing,
                adjustment: *adjustment,
            })
        }
        SocketMessage::AdjustWorkspacePadding(sizing, adjustment) => {
            Some(BuiltinAction::AdjustWorkspacePadding {
                sizing: *sizing,
                adjustment: *adjustment,
            })
        }
        SocketMessage::ToggleMouseFollowsFocus => Some(BuiltinAction::ToggleMouseFollowsFocus),
        SocketMessage::MouseFollowsFocus(enabled) => {
            Some(BuiltinAction::SetMouseFollowsFocus { enabled: *enabled })
        }
        SocketMessage::ToggleWindowContainerBehaviour => {
            Some(BuiltinAction::ToggleWindowContainerBehaviour)
        }
        SocketMessage::ToggleFloatOverride => Some(BuiltinAction::ToggleFloatOverride),
        SocketMessage::ToggleWorkspaceWindowContainerBehaviour => {
            Some(BuiltinAction::ToggleWorkspaceWindowContainerBehaviour)
        }
        SocketMessage::ToggleWorkspaceFloatOverride => {
            Some(BuiltinAction::ToggleWorkspaceFloatOverride)
        }
        SocketMessage::ToggleCrossMonitorMoveBehaviour => {
            Some(BuiltinAction::ToggleCrossMonitorMoveBehaviour)
        }
        SocketMessage::ToggleMonocleFocusBehaviour => {
            Some(BuiltinAction::ToggleMonocleFocusBehaviour)
        }
        SocketMessage::TogglePause => Some(BuiltinAction::TogglePause),
        SocketMessage::FocusedWorkspaceContainerPadding(size) => {
            Some(BuiltinAction::SetFocusedContainerPadding { size: *size })
        }
        SocketMessage::FocusedWorkspacePadding(size) => {
            Some(BuiltinAction::SetFocusedWorkspacePadding { size: *size })
        }
        SocketMessage::ContainerPadding(monitor, workspace, size) => {
            Some(BuiltinAction::SetContainerPadding {
                monitor: MonitorIndex::new(*monitor),
                workspace: WorkspaceIndex::new(*workspace),
                size: *size,
            })
        }
        SocketMessage::WorkspacePadding(monitor, workspace, size) => {
            Some(BuiltinAction::SetWorkspacePadding {
                monitor: MonitorIndex::new(*monitor),
                workspace: WorkspaceIndex::new(*workspace),
                size: *size,
            })
        }
        SocketMessage::WorkspaceTiling(monitor, workspace, tile) => {
            Some(BuiltinAction::SetWorkspaceTiling {
                monitor: MonitorIndex::new(*monitor),
                workspace: WorkspaceIndex::new(*workspace),
                tile: *tile,
            })
        }
        SocketMessage::WorkspaceLayout(monitor, workspace, layout) => {
            Some(BuiltinAction::SetMonitorWorkspaceLayout {
                monitor: MonitorIndex::new(*monitor),
                workspace: WorkspaceIndex::new(*workspace),
                layout: *layout,
            })
        }
        SocketMessage::EnsureWorkspaces(monitor, count) => Some(BuiltinAction::EnsureWorkspaces {
            monitor: MonitorIndex::new(*monitor),
            count: *count,
        }),
        SocketMessage::ClearWorkspaceLayoutRules(monitor, workspace) => {
            Some(BuiltinAction::ClearWorkspaceLayoutRules {
                monitor: MonitorIndex::new(*monitor),
                workspace: WorkspaceIndex::new(*workspace),
            })
        }
        SocketMessage::ScrollingLayoutColumns(columns) => {
            Some(BuiltinAction::SetScrollingColumns { columns: *columns })
        }
        SocketMessage::LockMonitorWorkspaceContainer(monitor, workspace, container) => {
            Some(BuiltinAction::LockContainer {
                monitor: MonitorIndex::new(*monitor),
                workspace: WorkspaceIndex::new(*workspace),
                container: ContainerIndex::new(*container),
            })
        }
        SocketMessage::UnlockMonitorWorkspaceContainer(monitor, workspace, container) => {
            Some(BuiltinAction::UnlockContainer {
                monitor: MonitorIndex::new(*monitor),
                workspace: WorkspaceIndex::new(*workspace),
                container: ContainerIndex::new(*container),
            })
        }
        SocketMessage::ToggleTitleBars => Some(BuiltinAction::ToggleTitleBars),
        SocketMessage::EnforceWorkspaceRules => Some(BuiltinAction::EnforceWorkspaceRules),
        SocketMessage::SessionFloatRule => Some(BuiltinAction::AddSessionFloatRule),
        SocketMessage::ClearSessionFloatRules => Some(BuiltinAction::ClearSessionFloatRules),
        SocketMessage::ResizeWindowEdge(direction, sizing) => {
            let signed = match sizing {
                Sizing::Increase => resize_delta,
                Sizing::Decrease => -resize_delta,
            };
            Some(BuiltinAction::ResizeWindowEdge {
                direction: *direction,
                delta: Pixels::new(signed).ok()?,
            })
        }
        SocketMessage::WindowHidingBehaviour(behaviour) => {
            Some(BuiltinAction::SetWindowHidingBehaviour {
                behaviour: *behaviour,
            })
        }
        SocketMessage::CrossMonitorMoveBehaviour(behaviour) => {
            Some(BuiltinAction::SetCrossMonitorMoveBehaviour {
                behaviour: *behaviour,
            })
        }
        SocketMessage::MonocleFocusBehaviour(behaviour) => {
            Some(BuiltinAction::SetMonocleFocusBehaviour {
                behaviour: *behaviour,
            })
        }
        SocketMessage::UnmanagedWindowOperationBehaviour(behaviour) => {
            Some(BuiltinAction::SetUnmanagedWindowOperationBehaviour {
                behaviour: *behaviour,
            })
        }
        SocketMessage::FocusFollowsMouse(implementation, enabled) => {
            Some(BuiltinAction::SetFocusFollowsMouse {
                implementation: *implementation,
                enabled: *enabled,
            })
        }
        SocketMessage::ToggleFocusFollowsMouse(implementation) => {
            Some(BuiltinAction::ToggleFocusFollowsMouse {
                implementation: *implementation,
            })
        }
        SocketMessage::WorkspaceLayoutRule(monitor, workspace, at_container_count, layout) => {
            Some(BuiltinAction::AddWorkspaceLayoutRule {
                monitor: MonitorIndex::new(*monitor),
                workspace: WorkspaceIndex::new(*workspace),
                at_container_count: *at_container_count,
                layout: *layout,
            })
        }
        SocketMessage::FocusNamedWorkspace(name) => Some(BuiltinAction::FocusNamedWorkspace {
            name: WorkspaceName::parse(name.clone()).ok()?,
        }),
        SocketMessage::MoveContainerToNamedWorkspace(name) => {
            Some(BuiltinAction::MoveContainerToNamedWorkspace {
                name: WorkspaceName::parse(name.clone()).ok()?,
            })
        }
        SocketMessage::SendContainerToNamedWorkspace(name) => {
            Some(BuiltinAction::SendContainerToNamedWorkspace {
                name: WorkspaceName::parse(name.clone()).ok()?,
            })
        }
        SocketMessage::NamedWorkspaceContainerPadding(name, size) => {
            Some(BuiltinAction::SetNamedWorkspaceContainerPadding {
                name: WorkspaceName::parse(name.clone()).ok()?,
                size: *size,
            })
        }
        SocketMessage::NamedWorkspacePadding(name, size) => {
            Some(BuiltinAction::SetNamedWorkspacePadding {
                name: WorkspaceName::parse(name.clone()).ok()?,
                size: *size,
            })
        }
        SocketMessage::NamedWorkspaceTiling(name, tile) => {
            Some(BuiltinAction::SetNamedWorkspaceTiling {
                name: WorkspaceName::parse(name.clone()).ok()?,
                tile: *tile,
            })
        }
        SocketMessage::NamedWorkspaceLayout(name, layout) => {
            Some(BuiltinAction::SetNamedWorkspaceLayout {
                name: WorkspaceName::parse(name.clone()).ok()?,
                layout: *layout,
            })
        }
        SocketMessage::NamedWorkspaceLayoutCustom(name, path) => {
            Some(BuiltinAction::SetNamedWorkspaceCustomLayout {
                name: WorkspaceName::parse(name.clone()).ok()?,
                path: WindowsPath::new(path.clone()).ok()?,
            })
        }
        SocketMessage::NamedWorkspaceLayoutRule(name, at_container_count, layout) => {
            Some(BuiltinAction::AddNamedWorkspaceLayoutRule {
                name: WorkspaceName::parse(name.clone()).ok()?,
                at_container_count: *at_container_count,
                layout: *layout,
            })
        }
        SocketMessage::NamedWorkspaceLayoutCustomRule(name, at_container_count, path) => {
            Some(BuiltinAction::AddNamedWorkspaceCustomLayoutRule {
                name: WorkspaceName::parse(name.clone()).ok()?,
                at_container_count: *at_container_count,
                path: WindowsPath::new(path.clone()).ok()?,
            })
        }
        SocketMessage::ClearNamedWorkspaceLayoutRules(name) => {
            Some(BuiltinAction::ClearNamedWorkspaceLayoutRules {
                name: WorkspaceName::parse(name.clone()).ok()?,
            })
        }
        SocketMessage::EnsureNamedWorkspaces(monitor, names) => {
            let names = names
                .iter()
                .map(|name| WorkspaceName::parse(name.clone()))
                .collect::<Result<Vec<_>, _>>()
                .ok()?;
            Some(BuiltinAction::EnsureNamedWorkspaces {
                monitor: MonitorIndex::new(*monitor),
                names,
            })
        }
        SocketMessage::WorkspaceName(monitor, workspace, name) => {
            Some(BuiltinAction::SetWorkspaceName {
                monitor: MonitorIndex::new(*monitor),
                workspace: WorkspaceIndex::new(*workspace),
                name: WorkspaceName::parse(name.clone()).ok()?,
            })
        }
        SocketMessage::LayoutRatios(columns, rows) => Some(BuiltinAction::SetLayoutRatios {
            columns: columns.clone(),
            rows: rows.clone(),
        }),
        SocketMessage::ChangeLayoutCustom(path) => Some(BuiltinAction::SetCustomLayout {
            path: WindowsPath::new(path.clone()).ok()?,
        }),
        SocketMessage::WorkspaceLayoutCustom(monitor, workspace, path) => {
            Some(BuiltinAction::SetWorkspaceCustomLayout {
                monitor: MonitorIndex::new(*monitor),
                workspace: WorkspaceIndex::new(*workspace),
                path: WindowsPath::new(path.clone()).ok()?,
            })
        }
        SocketMessage::WorkspaceLayoutCustomRule(monitor, workspace, at_container_count, path) => {
            Some(BuiltinAction::AddWorkspaceCustomLayoutRule {
                monitor: MonitorIndex::new(*monitor),
                workspace: WorkspaceIndex::new(*workspace),
                at_container_count: *at_container_count,
                path: WindowsPath::new(path.clone()).ok()?,
            })
        }
        SocketMessage::EagerFocus(exe) => Some(BuiltinAction::EagerFocus { exe: exe.clone() }),
        SocketMessage::RemoveTitleBar(identifier, id) => Some(BuiltinAction::RemoveTitleBar {
            identifier: *identifier,
            id: id.clone(),
        }),
        _ => None,
    }
}

pub fn adapt_action(
    message: &SocketMessage,
    resize_delta: i32,
) -> Result<Option<BuiltinAction>, SocketActionAdapterError> {
    let classification = classify(message);
    match (classification, to_builtin_action(message, resize_delta)) {
        (SocketMessageClass::Action, Some(action)) => Ok(Some(action)),
        (SocketMessageClass::Action, None) => Err(SocketActionAdapterError::InvalidParameters),
        (classification, Some(_)) => {
            Err(SocketActionAdapterError::ClassificationMismatch { classification })
        }
        (_, None) => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::DefaultLayout;
    use crate::core::OperationDirection;

    fn stamp(revision: u64) -> komorebi_protocol::StateStamp {
        komorebi_protocol::StateStamp::new(
            komorebi_protocol::ManagerEpoch::new([1; 16]).expect("test epoch is non-nil"),
            komorebi_protocol::Revision::try_from(revision).expect("test revision is nonzero"),
        )
    }

    #[test]
    fn migrated_socket_messages_become_the_same_builtin_action() {
        assert_eq!(
            to_builtin_action(&SocketMessage::FocusWindow(OperationDirection::Left), 50),
            Some(BuiltinAction::FocusWindow {
                direction: OperationDirection::Left,
            })
        );
        assert_eq!(
            classify(&SocketMessage::FocusWindow(OperationDirection::Left)),
            SocketMessageClass::Action
        );
        assert_eq!(
            to_builtin_action(&SocketMessage::MoveWindow(OperationDirection::Right), 50),
            Some(BuiltinAction::MoveWindow {
                direction: OperationDirection::Right,
            })
        );
        assert_eq!(
            to_builtin_action(&SocketMessage::ChangeLayout(DefaultLayout::Columns), 50),
            Some(BuiltinAction::SetWorkspaceLayout {
                workspace: WorkspaceSelector::FocusedAtExecution,
                layout: DefaultLayout::Columns,
            })
        );
        assert_eq!(
            to_builtin_action(&SocketMessage::ToggleFloat, 50),
            Some(BuiltinAction::ToggleWindowFloat {
                window: WindowSelector::FocusedAtExecution,
            })
        );
        assert_eq!(
            to_builtin_action(
                &SocketMessage::ResizeWindowAxis(
                    crate::core::Axis::Horizontal,
                    crate::core::Sizing::Decrease
                ),
                50
            ),
            Some(BuiltinAction::ResizeWindow {
                axis: crate::core::Axis::Horizontal,
                delta: crate::action::Pixels::new(-50).unwrap(),
            })
        );
        assert_eq!(
            to_builtin_action(
                &SocketMessage::ResizeWindowAxis(
                    crate::core::Axis::Horizontal,
                    crate::core::Sizing::Increase
                ),
                0
            ),
            None
        );
        assert_eq!(
            to_builtin_action(
                &SocketMessage::CycleFocusWindow(crate::core::CycleDirection::Next),
                50
            ),
            Some(BuiltinAction::CycleFocusWindow {
                direction: crate::core::CycleDirection::Next,
            })
        );
        assert_eq!(
            to_builtin_action(&SocketMessage::ToggleMonocle, 50),
            Some(BuiltinAction::ToggleWindowMonocle {
                window: WindowSelector::FocusedAtExecution,
            })
        );
        assert_eq!(
            to_builtin_action(
                &SocketMessage::CycleFocusWorkspace(crate::core::CycleDirection::Next),
                50
            ),
            Some(BuiltinAction::CycleFocusWorkspace {
                direction: crate::core::CycleDirection::Next,
            })
        );
        assert_eq!(
            to_builtin_action(&SocketMessage::FocusMonitorNumber(2), 50),
            Some(BuiltinAction::FocusMonitor {
                index: MonitorIndex::new(2),
            })
        );
        assert_eq!(
            to_builtin_action(&SocketMessage::FocusWorkspaceNumbers(3), 50),
            Some(BuiltinAction::FocusWorkspaceOnAllMonitors {
                index: WorkspaceIndex::new(3),
            })
        );
        assert_eq!(
            to_builtin_action(&SocketMessage::FocusMonitorWorkspaceNumber(1, 4), 50),
            Some(BuiltinAction::FocusMonitorWorkspace {
                monitor: MonitorIndex::new(1),
                workspace: WorkspaceIndex::new(4),
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
        assert_eq!(
            to_builtin_action(
                &SocketMessage::ResizeWindowEdge(
                    OperationDirection::Right,
                    crate::core::Sizing::Increase
                ),
                50
            ),
            Some(BuiltinAction::ResizeWindowEdge {
                direction: OperationDirection::Right,
                delta: crate::action::Pixels::new(50).unwrap(),
            })
        );
        assert_eq!(
            to_builtin_action(&SocketMessage::FocusNamedWorkspace("chat".into()), 50),
            Some(BuiltinAction::FocusNamedWorkspace {
                name: crate::action::WorkspaceName::parse("chat").unwrap(),
            })
        );
        assert_eq!(
            to_builtin_action(&SocketMessage::FocusNamedWorkspace("".into()), 50),
            None
        );
        assert_eq!(
            to_builtin_action(
                &SocketMessage::CrossMonitorMoveBehaviour(crate::core::MoveBehaviour::Insert),
                50
            ),
            Some(BuiltinAction::SetCrossMonitorMoveBehaviour {
                behaviour: crate::core::MoveBehaviour::Insert,
            })
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
        use crate::action::id::WindowId;
        use crate::action::invoke::ActionAdmission;
        use std::time::Instant;

        let message = SocketMessage::FocusWindow(OperationDirection::Left);
        let action = to_builtin_action(&message, 50).expect("focus-window is migrated");
        let mut state = CatalogState::new(ActionSnapshot {
            state: stamp(1),
            paused: false,
            focused_window: Some(WindowId::new(9)),
            directional_targets: [OperationDirection::Left].into(),
            current_layout: DefaultLayout::BSP,
            focused_window_floating: false,
            named_workspaces: Vec::new(),
            bindings: Vec::new(),
        });
        let admission = state.admit(
            InvokeAction {
                invocation_id: InvocationId::new(),
                expected_state: stamp(1),
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
            ActionAdmission::Committed { state, .. } if state == stamp(2)
        ));
    }

    #[test]
    fn adapter_separates_non_actions_from_invalid_actions() {
        assert_eq!(adapt_action(&SocketMessage::State, 50), Ok(None));
        assert_eq!(
            adapt_action(&SocketMessage::FocusNamedWorkspace(String::new()), 50),
            Err(SocketActionAdapterError::InvalidParameters)
        );
    }

    #[cfg(windows)]
    #[test]
    fn adapter_rejects_a_custom_layout_path_with_an_interior_nul() {
        use std::ffi::OsString;
        use std::os::windows::ffi::OsStringExt;
        use std::path::PathBuf;

        let path = PathBuf::from(OsString::from_wide(&[b'C' as u16, 0, b'x' as u16]));
        assert_eq!(
            adapt_action(&SocketMessage::ChangeLayoutCustom(path), 50),
            Err(SocketActionAdapterError::InvalidParameters)
        );
    }
}
