use serde::Deserialize;
use serde::Serialize;
use thiserror::Error;

use std::num::NonZeroUsize;

use crate::core::ApplicationIdentifier;
use crate::core::Axis;
use crate::core::BorderImplementation;
use crate::core::BorderOffset;
use crate::core::BorderStyle;
use crate::core::BorderWidth;
use crate::core::CycleDirection;
use crate::core::DefaultLayout;
use crate::core::FocusFollowsMouseImplementation;
use crate::core::HidingBehaviour;
use crate::core::MonocleFocusBehaviour;
use crate::core::MoveBehaviour;
use crate::core::OperationBehaviour;
use crate::core::OperationDirection;
use crate::core::ResizeStep;
use crate::core::Sizing;
use crate::core::TransparencyAlpha;
use crate::core::WindowKind;
use komorebi_themes::colour::Rgb;

use super::id::ActionId;
use super::index::ContainerIndex;
use super::index::MonitorIndex;
use super::index::StackIndex;
use super::index::WorkspaceIndex;
use super::path::WindowsPath;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum BuiltinActionKind {
    FocusWindow,
    MoveWindow,
    ResizeWindow,
    ResizeWindowByStep,
    SetWorkspaceLayout,
    ToggleWindowFloat,
    CycleFocusWindow,
    CycleMoveWindow,
    ToggleWindowMonocle,
    ToggleWindowMaximize,
    ToggleContainerLock,
    StackWindow,
    UnstackWindow,
    StackAll,
    UnstackAll,
    CycleStack,
    CycleStackIndex,
    FocusStackWindow,
    FocusWorkspace,
    CycleFocusWorkspace,
    CycleFocusEmptyWorkspace,
    FocusLastWorkspace,
    CloseWorkspace,
    FocusMonitor,
    CycleFocusMonitor,
    FocusMonitorAtCursor,
    FocusWorkspaceOnAllMonitors,
    FocusMonitorWorkspace,
    CloseWindow,
    MinimizeWindow,
    ForceFocus,
    PromoteContainer,
    PromoteContainerSwap,
    PromoteFocus,
    PromoteWindow,
    NewWorkspace,
    ToggleTiling,
    CycleLayout,
    FlipLayout,
    ToggleWorkspaceLayer,
    MoveContainerToLastWorkspace,
    SendContainerToLastWorkspace,
    MoveContainerToWorkspace,
    CycleMoveContainerToWorkspace,
    SendContainerToWorkspace,
    CycleSendContainerToWorkspace,
    MoveContainerToMonitor,
    CycleMoveContainerToMonitor,
    SendContainerToMonitor,
    CycleSendContainerToMonitor,
    MoveContainerToMonitorWorkspace,
    SendContainerToMonitorWorkspace,
    MoveWorkspaceToMonitor,
    CycleMoveWorkspaceToMonitor,
    SwapWorkspacesToMonitor,
    PreselectDirection,
    CancelPreselect,
    Retile,
    RetileWithResizeDimensions,
    ManageFocusedWindow,
    UnmanageFocusedWindow,
    AdjustContainerPadding,
    AdjustWorkspacePadding,
    ToggleMouseFollowsFocus,
    SetMouseFollowsFocus,
    ToggleWindowContainerBehaviour,
    ToggleFloatOverride,
    ToggleWorkspaceWindowContainerBehaviour,
    ToggleWorkspaceFloatOverride,
    ToggleCrossMonitorMoveBehaviour,
    ToggleMonocleFocusBehaviour,
    TogglePause,
    SetFocusedContainerPadding,
    SetFocusedWorkspacePadding,
    SetContainerPadding,
    SetWorkspacePadding,
    SetWorkspaceTiling,
    SetMonitorWorkspaceLayout,
    EnsureWorkspaces,
    ClearWorkspaceLayoutRules,
    SetScrollingColumns,
    LockContainer,
    UnlockContainer,
    ToggleTitleBars,
    EnforceWorkspaceRules,
    AddSessionFloatRule,
    ClearSessionFloatRules,
    ResizeWindowEdge,
    ResizeWindowEdgeByStep,
    SetWindowHidingBehaviour,
    SetCrossMonitorMoveBehaviour,
    SetMonocleFocusBehaviour,
    SetUnmanagedWindowOperationBehaviour,
    SetFocusFollowsMouse,
    ToggleFocusFollowsMouse,
    AddWorkspaceLayoutRule,
    FocusNamedWorkspace,
    MoveContainerToNamedWorkspace,
    SendContainerToNamedWorkspace,
    SetNamedWorkspaceContainerPadding,
    SetNamedWorkspacePadding,
    SetNamedWorkspaceTiling,
    SetNamedWorkspaceLayout,
    SetNamedWorkspaceCustomLayout,
    AddNamedWorkspaceLayoutRule,
    AddNamedWorkspaceCustomLayoutRule,
    ClearNamedWorkspaceLayoutRules,
    EnsureNamedWorkspaces,
    SetWorkspaceName,
    SetLayoutRatios,
    SetCustomLayout,
    SetWorkspaceCustomLayout,
    AddWorkspaceCustomLayoutRule,
    EagerFocus,
    RemoveTitleBar,
    SetResizeStep,
    SetTransparencyEnabled,
    ToggleTransparency,
    SetTransparencyAlpha,
    SetBorderEnabled,
    SetBorderColour,
    SetBorderWidth,
    SetBorderOffset,
    SetBorderStyle,
    SetBorderImplementation,
}

impl BuiltinActionKind {
    pub const ALL: [Self; 125] = [
        Self::FocusWindow,
        Self::MoveWindow,
        Self::ResizeWindow,
        Self::ResizeWindowByStep,
        Self::SetWorkspaceLayout,
        Self::ToggleWindowFloat,
        Self::CycleFocusWindow,
        Self::CycleMoveWindow,
        Self::ToggleWindowMonocle,
        Self::ToggleWindowMaximize,
        Self::ToggleContainerLock,
        Self::StackWindow,
        Self::UnstackWindow,
        Self::StackAll,
        Self::UnstackAll,
        Self::CycleStack,
        Self::CycleStackIndex,
        Self::FocusStackWindow,
        Self::FocusWorkspace,
        Self::CycleFocusWorkspace,
        Self::CycleFocusEmptyWorkspace,
        Self::FocusLastWorkspace,
        Self::CloseWorkspace,
        Self::FocusMonitor,
        Self::CycleFocusMonitor,
        Self::FocusMonitorAtCursor,
        Self::FocusWorkspaceOnAllMonitors,
        Self::FocusMonitorWorkspace,
        Self::CloseWindow,
        Self::MinimizeWindow,
        Self::ForceFocus,
        Self::PromoteContainer,
        Self::PromoteContainerSwap,
        Self::PromoteFocus,
        Self::PromoteWindow,
        Self::NewWorkspace,
        Self::ToggleTiling,
        Self::CycleLayout,
        Self::FlipLayout,
        Self::ToggleWorkspaceLayer,
        Self::MoveContainerToLastWorkspace,
        Self::SendContainerToLastWorkspace,
        Self::MoveContainerToWorkspace,
        Self::CycleMoveContainerToWorkspace,
        Self::SendContainerToWorkspace,
        Self::CycleSendContainerToWorkspace,
        Self::MoveContainerToMonitor,
        Self::CycleMoveContainerToMonitor,
        Self::SendContainerToMonitor,
        Self::CycleSendContainerToMonitor,
        Self::MoveContainerToMonitorWorkspace,
        Self::SendContainerToMonitorWorkspace,
        Self::MoveWorkspaceToMonitor,
        Self::CycleMoveWorkspaceToMonitor,
        Self::SwapWorkspacesToMonitor,
        Self::PreselectDirection,
        Self::CancelPreselect,
        Self::Retile,
        Self::RetileWithResizeDimensions,
        Self::ManageFocusedWindow,
        Self::UnmanageFocusedWindow,
        Self::AdjustContainerPadding,
        Self::AdjustWorkspacePadding,
        Self::ToggleMouseFollowsFocus,
        Self::SetMouseFollowsFocus,
        Self::ToggleWindowContainerBehaviour,
        Self::ToggleFloatOverride,
        Self::ToggleWorkspaceWindowContainerBehaviour,
        Self::ToggleWorkspaceFloatOverride,
        Self::ToggleCrossMonitorMoveBehaviour,
        Self::ToggleMonocleFocusBehaviour,
        Self::TogglePause,
        Self::SetFocusedContainerPadding,
        Self::SetFocusedWorkspacePadding,
        Self::SetContainerPadding,
        Self::SetWorkspacePadding,
        Self::SetWorkspaceTiling,
        Self::SetMonitorWorkspaceLayout,
        Self::EnsureWorkspaces,
        Self::ClearWorkspaceLayoutRules,
        Self::SetScrollingColumns,
        Self::LockContainer,
        Self::UnlockContainer,
        Self::ToggleTitleBars,
        Self::EnforceWorkspaceRules,
        Self::AddSessionFloatRule,
        Self::ClearSessionFloatRules,
        Self::ResizeWindowEdge,
        Self::ResizeWindowEdgeByStep,
        Self::SetWindowHidingBehaviour,
        Self::SetCrossMonitorMoveBehaviour,
        Self::SetMonocleFocusBehaviour,
        Self::SetUnmanagedWindowOperationBehaviour,
        Self::SetFocusFollowsMouse,
        Self::ToggleFocusFollowsMouse,
        Self::AddWorkspaceLayoutRule,
        Self::FocusNamedWorkspace,
        Self::MoveContainerToNamedWorkspace,
        Self::SendContainerToNamedWorkspace,
        Self::SetNamedWorkspaceContainerPadding,
        Self::SetNamedWorkspacePadding,
        Self::SetNamedWorkspaceTiling,
        Self::SetNamedWorkspaceLayout,
        Self::SetNamedWorkspaceCustomLayout,
        Self::AddNamedWorkspaceLayoutRule,
        Self::AddNamedWorkspaceCustomLayoutRule,
        Self::ClearNamedWorkspaceLayoutRules,
        Self::EnsureNamedWorkspaces,
        Self::SetWorkspaceName,
        Self::SetLayoutRatios,
        Self::SetCustomLayout,
        Self::SetWorkspaceCustomLayout,
        Self::AddWorkspaceCustomLayoutRule,
        Self::EagerFocus,
        Self::RemoveTitleBar,
        Self::SetResizeStep,
        Self::SetTransparencyEnabled,
        Self::ToggleTransparency,
        Self::SetTransparencyAlpha,
        Self::SetBorderEnabled,
        Self::SetBorderColour,
        Self::SetBorderWidth,
        Self::SetBorderOffset,
        Self::SetBorderStyle,
        Self::SetBorderImplementation,
    ];

    #[must_use]
    pub const fn id(self) -> ActionId {
        match self {
            Self::FocusWindow => ActionId::FOCUS_WINDOW,
            Self::MoveWindow => ActionId::MOVE_WINDOW,
            Self::ResizeWindow => ActionId::RESIZE_WINDOW,
            Self::ResizeWindowByStep => ActionId::RESIZE_WINDOW_BY_STEP,
            Self::SetWorkspaceLayout => ActionId::SET_WORKSPACE_LAYOUT,
            Self::ToggleWindowFloat => ActionId::TOGGLE_WINDOW_FLOAT,
            Self::CycleFocusWindow => ActionId::CYCLE_FOCUS_WINDOW,
            Self::CycleMoveWindow => ActionId::CYCLE_MOVE_WINDOW,
            Self::ToggleWindowMonocle => ActionId::TOGGLE_WINDOW_MONOCLE,
            Self::ToggleWindowMaximize => ActionId::TOGGLE_WINDOW_MAXIMIZE,
            Self::ToggleContainerLock => ActionId::TOGGLE_CONTAINER_LOCK,
            Self::StackWindow => ActionId::STACK_WINDOW,
            Self::UnstackWindow => ActionId::UNSTACK_WINDOW,
            Self::StackAll => ActionId::STACK_ALL,
            Self::UnstackAll => ActionId::UNSTACK_ALL,
            Self::CycleStack => ActionId::CYCLE_STACK,
            Self::CycleStackIndex => ActionId::CYCLE_STACK_INDEX,
            Self::FocusStackWindow => ActionId::FOCUS_STACK_WINDOW,
            Self::FocusWorkspace => ActionId::FOCUS_WORKSPACE,
            Self::CycleFocusWorkspace => ActionId::CYCLE_FOCUS_WORKSPACE,
            Self::CycleFocusEmptyWorkspace => ActionId::CYCLE_FOCUS_EMPTY_WORKSPACE,
            Self::FocusLastWorkspace => ActionId::FOCUS_LAST_WORKSPACE,
            Self::CloseWorkspace => ActionId::CLOSE_WORKSPACE,
            Self::FocusMonitor => ActionId::FOCUS_MONITOR,
            Self::CycleFocusMonitor => ActionId::CYCLE_FOCUS_MONITOR,
            Self::FocusMonitorAtCursor => ActionId::FOCUS_MONITOR_AT_CURSOR,
            Self::FocusWorkspaceOnAllMonitors => ActionId::FOCUS_WORKSPACE_ON_ALL_MONITORS,
            Self::FocusMonitorWorkspace => ActionId::FOCUS_MONITOR_WORKSPACE,
            Self::CloseWindow => ActionId::CLOSE_WINDOW,
            Self::MinimizeWindow => ActionId::MINIMIZE_WINDOW,
            Self::ForceFocus => ActionId::FORCE_FOCUS,
            Self::PromoteContainer => ActionId::PROMOTE_CONTAINER,
            Self::PromoteContainerSwap => ActionId::PROMOTE_CONTAINER_SWAP,
            Self::PromoteFocus => ActionId::PROMOTE_FOCUS,
            Self::PromoteWindow => ActionId::PROMOTE_WINDOW,
            Self::NewWorkspace => ActionId::NEW_WORKSPACE,
            Self::ToggleTiling => ActionId::TOGGLE_TILING,
            Self::CycleLayout => ActionId::CYCLE_LAYOUT,
            Self::FlipLayout => ActionId::FLIP_LAYOUT,
            Self::ToggleWorkspaceLayer => ActionId::TOGGLE_WORKSPACE_LAYER,
            Self::MoveContainerToLastWorkspace => ActionId::MOVE_CONTAINER_TO_LAST_WORKSPACE,
            Self::SendContainerToLastWorkspace => ActionId::SEND_CONTAINER_TO_LAST_WORKSPACE,
            Self::MoveContainerToWorkspace => ActionId::MOVE_CONTAINER_TO_WORKSPACE,
            Self::CycleMoveContainerToWorkspace => ActionId::CYCLE_MOVE_CONTAINER_TO_WORKSPACE,
            Self::SendContainerToWorkspace => ActionId::SEND_CONTAINER_TO_WORKSPACE,
            Self::CycleSendContainerToWorkspace => ActionId::CYCLE_SEND_CONTAINER_TO_WORKSPACE,
            Self::MoveContainerToMonitor => ActionId::MOVE_CONTAINER_TO_MONITOR,
            Self::CycleMoveContainerToMonitor => ActionId::CYCLE_MOVE_CONTAINER_TO_MONITOR,
            Self::SendContainerToMonitor => ActionId::SEND_CONTAINER_TO_MONITOR,
            Self::CycleSendContainerToMonitor => ActionId::CYCLE_SEND_CONTAINER_TO_MONITOR,
            Self::MoveContainerToMonitorWorkspace => ActionId::MOVE_CONTAINER_TO_MONITOR_WORKSPACE,
            Self::SendContainerToMonitorWorkspace => ActionId::SEND_CONTAINER_TO_MONITOR_WORKSPACE,
            Self::MoveWorkspaceToMonitor => ActionId::MOVE_WORKSPACE_TO_MONITOR,
            Self::CycleMoveWorkspaceToMonitor => ActionId::CYCLE_MOVE_WORKSPACE_TO_MONITOR,
            Self::SwapWorkspacesToMonitor => ActionId::SWAP_WORKSPACES_TO_MONITOR,
            Self::PreselectDirection => ActionId::PRESELECT_DIRECTION,
            Self::CancelPreselect => ActionId::CANCEL_PRESELECT,
            Self::Retile => ActionId::RETILE,
            Self::RetileWithResizeDimensions => ActionId::RETILE_WITH_RESIZE_DIMENSIONS,
            Self::ManageFocusedWindow => ActionId::MANAGE_FOCUSED_WINDOW,
            Self::UnmanageFocusedWindow => ActionId::UNMANAGE_FOCUSED_WINDOW,
            Self::AdjustContainerPadding => ActionId::ADJUST_CONTAINER_PADDING,
            Self::AdjustWorkspacePadding => ActionId::ADJUST_WORKSPACE_PADDING,
            Self::ToggleMouseFollowsFocus => ActionId::TOGGLE_MOUSE_FOLLOWS_FOCUS,
            Self::SetMouseFollowsFocus => ActionId::SET_MOUSE_FOLLOWS_FOCUS,
            Self::ToggleWindowContainerBehaviour => ActionId::TOGGLE_WINDOW_CONTAINER_BEHAVIOUR,
            Self::ToggleFloatOverride => ActionId::TOGGLE_FLOAT_OVERRIDE,
            Self::ToggleWorkspaceWindowContainerBehaviour => {
                ActionId::TOGGLE_WORKSPACE_WINDOW_CONTAINER_BEHAVIOUR
            }
            Self::ToggleWorkspaceFloatOverride => ActionId::TOGGLE_WORKSPACE_FLOAT_OVERRIDE,
            Self::ToggleCrossMonitorMoveBehaviour => ActionId::TOGGLE_CROSS_MONITOR_MOVE_BEHAVIOUR,
            Self::ToggleMonocleFocusBehaviour => ActionId::TOGGLE_MONOCLE_FOCUS_BEHAVIOUR,
            Self::TogglePause => ActionId::TOGGLE_PAUSE,
            Self::SetFocusedContainerPadding => ActionId::SET_FOCUSED_CONTAINER_PADDING,
            Self::SetFocusedWorkspacePadding => ActionId::SET_FOCUSED_WORKSPACE_PADDING,
            Self::SetContainerPadding => ActionId::SET_CONTAINER_PADDING,
            Self::SetWorkspacePadding => ActionId::SET_WORKSPACE_PADDING,
            Self::SetWorkspaceTiling => ActionId::SET_WORKSPACE_TILING,
            Self::SetMonitorWorkspaceLayout => ActionId::SET_MONITOR_WORKSPACE_LAYOUT,
            Self::EnsureWorkspaces => ActionId::ENSURE_WORKSPACES,
            Self::ClearWorkspaceLayoutRules => ActionId::CLEAR_WORKSPACE_LAYOUT_RULES,
            Self::SetScrollingColumns => ActionId::SET_SCROLLING_COLUMNS,
            Self::LockContainer => ActionId::LOCK_CONTAINER,
            Self::UnlockContainer => ActionId::UNLOCK_CONTAINER,
            Self::ToggleTitleBars => ActionId::TOGGLE_TITLE_BARS,
            Self::EnforceWorkspaceRules => ActionId::ENFORCE_WORKSPACE_RULES,
            Self::AddSessionFloatRule => ActionId::ADD_SESSION_FLOAT_RULE,
            Self::ClearSessionFloatRules => ActionId::CLEAR_SESSION_FLOAT_RULES,
            Self::ResizeWindowEdge => ActionId::RESIZE_WINDOW_EDGE,
            Self::ResizeWindowEdgeByStep => ActionId::RESIZE_WINDOW_EDGE_BY_STEP,
            Self::SetWindowHidingBehaviour => ActionId::SET_WINDOW_HIDING_BEHAVIOUR,
            Self::SetCrossMonitorMoveBehaviour => ActionId::SET_CROSS_MONITOR_MOVE_BEHAVIOUR,
            Self::SetMonocleFocusBehaviour => ActionId::SET_MONOCLE_FOCUS_BEHAVIOUR,
            Self::SetUnmanagedWindowOperationBehaviour => {
                ActionId::SET_UNMANAGED_WINDOW_OPERATION_BEHAVIOUR
            }
            Self::SetFocusFollowsMouse => ActionId::SET_FOCUS_FOLLOWS_MOUSE,
            Self::ToggleFocusFollowsMouse => ActionId::TOGGLE_FOCUS_FOLLOWS_MOUSE,
            Self::AddWorkspaceLayoutRule => ActionId::ADD_WORKSPACE_LAYOUT_RULE,
            Self::FocusNamedWorkspace => ActionId::FOCUS_NAMED_WORKSPACE,
            Self::MoveContainerToNamedWorkspace => ActionId::MOVE_CONTAINER_TO_NAMED_WORKSPACE,
            Self::SendContainerToNamedWorkspace => ActionId::SEND_CONTAINER_TO_NAMED_WORKSPACE,
            Self::SetNamedWorkspaceContainerPadding => {
                ActionId::SET_NAMED_WORKSPACE_CONTAINER_PADDING
            }
            Self::SetNamedWorkspacePadding => ActionId::SET_NAMED_WORKSPACE_PADDING,
            Self::SetNamedWorkspaceTiling => ActionId::SET_NAMED_WORKSPACE_TILING,
            Self::SetNamedWorkspaceLayout => ActionId::SET_NAMED_WORKSPACE_LAYOUT,
            Self::SetNamedWorkspaceCustomLayout => ActionId::SET_NAMED_WORKSPACE_CUSTOM_LAYOUT,
            Self::AddNamedWorkspaceLayoutRule => ActionId::ADD_NAMED_WORKSPACE_LAYOUT_RULE,
            Self::AddNamedWorkspaceCustomLayoutRule => {
                ActionId::ADD_NAMED_WORKSPACE_CUSTOM_LAYOUT_RULE
            }
            Self::ClearNamedWorkspaceLayoutRules => ActionId::CLEAR_NAMED_WORKSPACE_LAYOUT_RULES,
            Self::EnsureNamedWorkspaces => ActionId::ENSURE_NAMED_WORKSPACES,
            Self::SetWorkspaceName => ActionId::SET_WORKSPACE_NAME,
            Self::SetLayoutRatios => ActionId::SET_LAYOUT_RATIOS,
            Self::SetCustomLayout => ActionId::SET_CUSTOM_LAYOUT,
            Self::SetWorkspaceCustomLayout => ActionId::SET_WORKSPACE_CUSTOM_LAYOUT,
            Self::AddWorkspaceCustomLayoutRule => ActionId::ADD_WORKSPACE_CUSTOM_LAYOUT_RULE,
            Self::EagerFocus => ActionId::EAGER_FOCUS,
            Self::RemoveTitleBar => ActionId::REMOVE_TITLE_BAR,
            Self::SetResizeStep => ActionId::SET_RESIZE_STEP,
            Self::SetTransparencyEnabled => ActionId::SET_TRANSPARENCY_ENABLED,
            Self::ToggleTransparency => ActionId::TOGGLE_TRANSPARENCY,
            Self::SetTransparencyAlpha => ActionId::SET_TRANSPARENCY_ALPHA,
            Self::SetBorderEnabled => ActionId::SET_BORDER_ENABLED,
            Self::SetBorderColour => ActionId::SET_BORDER_COLOUR,
            Self::SetBorderWidth => ActionId::SET_BORDER_WIDTH,
            Self::SetBorderOffset => ActionId::SET_BORDER_OFFSET,
            Self::SetBorderStyle => ActionId::SET_BORDER_STYLE,
            Self::SetBorderImplementation => ActionId::SET_BORDER_IMPLEMENTATION,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum WorkspaceSelector {
    FocusedAtExecution,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum WindowSelector {
    FocusedAtExecution,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Pixels(i32);

impl Pixels {
    pub fn new(value: i32) -> Result<Self, PixelsError> {
        if value == 0 {
            return Err(PixelsError::Zero);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub const fn get(self) -> i32 {
        self.0
    }

    #[must_use]
    pub const fn from_resize_step(step: ResizeStep, sizing: Sizing) -> Self {
        match sizing {
            Sizing::Increase => Self(step.get()),
            Sizing::Decrease => Self(step.negative()),
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Error)]
pub enum PixelsError {
    #[error("pixel delta must be non-zero")]
    Zero,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct WorkspaceName(String);

impl WorkspaceName {
    pub fn parse(name: impl Into<String>) -> Result<Self, WorkspaceNameError> {
        let name = name.into();
        if name.trim().is_empty() {
            return Err(WorkspaceNameError::Empty);
        }
        Ok(Self(name))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[must_use]
    pub fn into_string(self) -> String {
        self.0
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Error)]
pub enum WorkspaceNameError {
    #[error("workspace name must not be empty")]
    Empty,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum BuiltinAction {
    FocusWindow {
        direction: OperationDirection,
    },
    MoveWindow {
        direction: OperationDirection,
    },
    ResizeWindow {
        axis: Axis,
        delta: Pixels,
    },
    ResizeWindowByStep {
        axis: Axis,
        sizing: Sizing,
    },
    SetWorkspaceLayout {
        workspace: WorkspaceSelector,
        layout: DefaultLayout,
    },
    ToggleWindowFloat {
        window: WindowSelector,
    },
    CycleFocusWindow {
        direction: CycleDirection,
    },
    CycleMoveWindow {
        direction: CycleDirection,
    },
    ToggleWindowMonocle {
        window: WindowSelector,
    },
    ToggleWindowMaximize {
        window: WindowSelector,
    },
    ToggleContainerLock {
        window: WindowSelector,
    },
    StackWindow {
        direction: OperationDirection,
    },
    UnstackWindow {
        window: WindowSelector,
    },
    StackAll,
    UnstackAll,
    CycleStack {
        direction: CycleDirection,
    },
    CycleStackIndex {
        direction: CycleDirection,
    },
    FocusStackWindow {
        index: StackIndex,
    },
    FocusWorkspace {
        index: WorkspaceIndex,
    },
    CycleFocusWorkspace {
        direction: CycleDirection,
    },
    CycleFocusEmptyWorkspace {
        direction: CycleDirection,
    },
    FocusLastWorkspace,
    CloseWorkspace,
    FocusMonitor {
        index: MonitorIndex,
    },
    CycleFocusMonitor {
        direction: CycleDirection,
    },
    FocusMonitorAtCursor,
    FocusWorkspaceOnAllMonitors {
        index: WorkspaceIndex,
    },
    FocusMonitorWorkspace {
        monitor: MonitorIndex,
        workspace: WorkspaceIndex,
    },
    CloseWindow {
        window: WindowSelector,
    },
    MinimizeWindow {
        window: WindowSelector,
    },
    ForceFocus {
        window: WindowSelector,
    },
    PromoteContainer,
    PromoteContainerSwap,
    PromoteFocus,
    PromoteWindow {
        direction: OperationDirection,
    },
    NewWorkspace,
    ToggleTiling,
    CycleLayout {
        direction: CycleDirection,
    },
    FlipLayout {
        axis: Axis,
    },
    ToggleWorkspaceLayer,
    MoveContainerToLastWorkspace,
    SendContainerToLastWorkspace,
    MoveContainerToWorkspace {
        index: WorkspaceIndex,
    },
    CycleMoveContainerToWorkspace {
        direction: CycleDirection,
    },
    SendContainerToWorkspace {
        index: WorkspaceIndex,
    },
    CycleSendContainerToWorkspace {
        direction: CycleDirection,
    },
    MoveContainerToMonitor {
        index: MonitorIndex,
    },
    CycleMoveContainerToMonitor {
        direction: CycleDirection,
    },
    SendContainerToMonitor {
        index: MonitorIndex,
    },
    CycleSendContainerToMonitor {
        direction: CycleDirection,
    },
    MoveContainerToMonitorWorkspace {
        monitor: MonitorIndex,
        workspace: WorkspaceIndex,
    },
    SendContainerToMonitorWorkspace {
        monitor: MonitorIndex,
        workspace: WorkspaceIndex,
    },
    MoveWorkspaceToMonitor {
        index: MonitorIndex,
    },
    CycleMoveWorkspaceToMonitor {
        direction: CycleDirection,
    },
    SwapWorkspacesToMonitor {
        index: MonitorIndex,
    },
    PreselectDirection {
        direction: OperationDirection,
    },
    CancelPreselect,
    Retile,
    RetileWithResizeDimensions,
    ManageFocusedWindow,
    UnmanageFocusedWindow,
    AdjustContainerPadding {
        sizing: Sizing,
        adjustment: i32,
    },
    AdjustWorkspacePadding {
        sizing: Sizing,
        adjustment: i32,
    },
    ToggleMouseFollowsFocus,
    SetMouseFollowsFocus {
        enabled: bool,
    },
    ToggleWindowContainerBehaviour,
    ToggleFloatOverride,
    ToggleWorkspaceWindowContainerBehaviour,
    ToggleWorkspaceFloatOverride,
    ToggleCrossMonitorMoveBehaviour,
    ToggleMonocleFocusBehaviour,
    TogglePause,
    SetFocusedContainerPadding {
        size: i32,
    },
    SetFocusedWorkspacePadding {
        size: i32,
    },
    SetContainerPadding {
        monitor: MonitorIndex,
        workspace: WorkspaceIndex,
        size: i32,
    },
    SetWorkspacePadding {
        monitor: MonitorIndex,
        workspace: WorkspaceIndex,
        size: i32,
    },
    SetWorkspaceTiling {
        monitor: MonitorIndex,
        workspace: WorkspaceIndex,
        tile: bool,
    },
    SetMonitorWorkspaceLayout {
        monitor: MonitorIndex,
        workspace: WorkspaceIndex,
        layout: DefaultLayout,
    },
    EnsureWorkspaces {
        monitor: MonitorIndex,
        count: usize,
    },
    ClearWorkspaceLayoutRules {
        monitor: MonitorIndex,
        workspace: WorkspaceIndex,
    },
    SetScrollingColumns {
        columns: NonZeroUsize,
    },
    LockContainer {
        monitor: MonitorIndex,
        workspace: WorkspaceIndex,
        container: ContainerIndex,
    },
    UnlockContainer {
        monitor: MonitorIndex,
        workspace: WorkspaceIndex,
        container: ContainerIndex,
    },
    ToggleTitleBars,
    EnforceWorkspaceRules,
    AddSessionFloatRule,
    ClearSessionFloatRules,
    ResizeWindowEdge {
        direction: OperationDirection,
        delta: Pixels,
    },
    ResizeWindowEdgeByStep {
        direction: OperationDirection,
        sizing: Sizing,
    },
    SetWindowHidingBehaviour {
        behaviour: HidingBehaviour,
    },
    SetCrossMonitorMoveBehaviour {
        behaviour: MoveBehaviour,
    },
    SetMonocleFocusBehaviour {
        behaviour: MonocleFocusBehaviour,
    },
    SetUnmanagedWindowOperationBehaviour {
        behaviour: OperationBehaviour,
    },
    SetFocusFollowsMouse {
        implementation: FocusFollowsMouseImplementation,
        enabled: bool,
    },
    ToggleFocusFollowsMouse {
        implementation: FocusFollowsMouseImplementation,
    },
    AddWorkspaceLayoutRule {
        monitor: MonitorIndex,
        workspace: WorkspaceIndex,
        at_container_count: usize,
        layout: DefaultLayout,
    },
    FocusNamedWorkspace {
        name: WorkspaceName,
    },
    MoveContainerToNamedWorkspace {
        name: WorkspaceName,
    },
    SendContainerToNamedWorkspace {
        name: WorkspaceName,
    },
    SetNamedWorkspaceContainerPadding {
        name: WorkspaceName,
        size: i32,
    },
    SetNamedWorkspacePadding {
        name: WorkspaceName,
        size: i32,
    },
    SetNamedWorkspaceTiling {
        name: WorkspaceName,
        tile: bool,
    },
    SetNamedWorkspaceLayout {
        name: WorkspaceName,
        layout: DefaultLayout,
    },
    SetNamedWorkspaceCustomLayout {
        name: WorkspaceName,
        path: WindowsPath,
    },
    AddNamedWorkspaceLayoutRule {
        name: WorkspaceName,
        at_container_count: usize,
        layout: DefaultLayout,
    },
    AddNamedWorkspaceCustomLayoutRule {
        name: WorkspaceName,
        at_container_count: usize,
        path: WindowsPath,
    },
    ClearNamedWorkspaceLayoutRules {
        name: WorkspaceName,
    },
    EnsureNamedWorkspaces {
        monitor: MonitorIndex,
        names: Vec<WorkspaceName>,
    },
    SetWorkspaceName {
        monitor: MonitorIndex,
        workspace: WorkspaceIndex,
        name: WorkspaceName,
    },
    SetLayoutRatios {
        columns: Option<Vec<f32>>,
        rows: Option<Vec<f32>>,
    },
    SetCustomLayout {
        path: WindowsPath,
    },
    SetWorkspaceCustomLayout {
        monitor: MonitorIndex,
        workspace: WorkspaceIndex,
        path: WindowsPath,
    },
    AddWorkspaceCustomLayoutRule {
        monitor: MonitorIndex,
        workspace: WorkspaceIndex,
        at_container_count: usize,
        path: WindowsPath,
    },
    EagerFocus {
        exe: String,
    },
    RemoveTitleBar {
        identifier: ApplicationIdentifier,
        id: String,
    },
    SetResizeStep {
        step: ResizeStep,
    },
    SetTransparencyEnabled {
        enabled: bool,
    },
    ToggleTransparency,
    SetTransparencyAlpha {
        alpha: TransparencyAlpha,
    },
    SetBorderEnabled {
        enabled: bool,
    },
    SetBorderColour {
        window_kind: WindowKind,
        colour: Rgb,
    },
    SetBorderWidth {
        width: BorderWidth,
    },
    SetBorderOffset {
        offset: BorderOffset,
    },
    SetBorderStyle {
        style: BorderStyle,
    },
    SetBorderImplementation {
        implementation: BorderImplementation,
    },
}

impl BuiltinAction {
    #[must_use]
    pub const fn kind(&self) -> BuiltinActionKind {
        match self {
            Self::FocusWindow { .. } => BuiltinActionKind::FocusWindow,
            Self::MoveWindow { .. } => BuiltinActionKind::MoveWindow,
            Self::ResizeWindow { .. } => BuiltinActionKind::ResizeWindow,
            Self::ResizeWindowByStep { .. } => BuiltinActionKind::ResizeWindowByStep,
            Self::SetWorkspaceLayout { .. } => BuiltinActionKind::SetWorkspaceLayout,
            Self::ToggleWindowFloat { .. } => BuiltinActionKind::ToggleWindowFloat,
            Self::CycleFocusWindow { .. } => BuiltinActionKind::CycleFocusWindow,
            Self::CycleMoveWindow { .. } => BuiltinActionKind::CycleMoveWindow,
            Self::ToggleWindowMonocle { .. } => BuiltinActionKind::ToggleWindowMonocle,
            Self::ToggleWindowMaximize { .. } => BuiltinActionKind::ToggleWindowMaximize,
            Self::ToggleContainerLock { .. } => BuiltinActionKind::ToggleContainerLock,
            Self::StackWindow { .. } => BuiltinActionKind::StackWindow,
            Self::UnstackWindow { .. } => BuiltinActionKind::UnstackWindow,
            Self::StackAll => BuiltinActionKind::StackAll,
            Self::UnstackAll => BuiltinActionKind::UnstackAll,
            Self::CycleStack { .. } => BuiltinActionKind::CycleStack,
            Self::CycleStackIndex { .. } => BuiltinActionKind::CycleStackIndex,
            Self::FocusStackWindow { .. } => BuiltinActionKind::FocusStackWindow,
            Self::FocusWorkspace { .. } => BuiltinActionKind::FocusWorkspace,
            Self::CycleFocusWorkspace { .. } => BuiltinActionKind::CycleFocusWorkspace,
            Self::CycleFocusEmptyWorkspace { .. } => BuiltinActionKind::CycleFocusEmptyWorkspace,
            Self::FocusLastWorkspace => BuiltinActionKind::FocusLastWorkspace,
            Self::CloseWorkspace => BuiltinActionKind::CloseWorkspace,
            Self::FocusMonitor { .. } => BuiltinActionKind::FocusMonitor,
            Self::CycleFocusMonitor { .. } => BuiltinActionKind::CycleFocusMonitor,
            Self::FocusMonitorAtCursor => BuiltinActionKind::FocusMonitorAtCursor,
            Self::FocusWorkspaceOnAllMonitors { .. } => {
                BuiltinActionKind::FocusWorkspaceOnAllMonitors
            }
            Self::FocusMonitorWorkspace { .. } => BuiltinActionKind::FocusMonitorWorkspace,
            Self::CloseWindow { .. } => BuiltinActionKind::CloseWindow,
            Self::MinimizeWindow { .. } => BuiltinActionKind::MinimizeWindow,
            Self::ForceFocus { .. } => BuiltinActionKind::ForceFocus,
            Self::PromoteContainer => BuiltinActionKind::PromoteContainer,
            Self::PromoteContainerSwap => BuiltinActionKind::PromoteContainerSwap,
            Self::PromoteFocus => BuiltinActionKind::PromoteFocus,
            Self::PromoteWindow { .. } => BuiltinActionKind::PromoteWindow,
            Self::NewWorkspace => BuiltinActionKind::NewWorkspace,
            Self::ToggleTiling => BuiltinActionKind::ToggleTiling,
            Self::CycleLayout { .. } => BuiltinActionKind::CycleLayout,
            Self::FlipLayout { .. } => BuiltinActionKind::FlipLayout,
            Self::ToggleWorkspaceLayer => BuiltinActionKind::ToggleWorkspaceLayer,
            Self::MoveContainerToLastWorkspace => BuiltinActionKind::MoveContainerToLastWorkspace,
            Self::SendContainerToLastWorkspace => BuiltinActionKind::SendContainerToLastWorkspace,
            Self::MoveContainerToWorkspace { .. } => BuiltinActionKind::MoveContainerToWorkspace,
            Self::CycleMoveContainerToWorkspace { .. } => {
                BuiltinActionKind::CycleMoveContainerToWorkspace
            }
            Self::SendContainerToWorkspace { .. } => BuiltinActionKind::SendContainerToWorkspace,
            Self::CycleSendContainerToWorkspace { .. } => {
                BuiltinActionKind::CycleSendContainerToWorkspace
            }
            Self::MoveContainerToMonitor { .. } => BuiltinActionKind::MoveContainerToMonitor,
            Self::CycleMoveContainerToMonitor { .. } => {
                BuiltinActionKind::CycleMoveContainerToMonitor
            }
            Self::SendContainerToMonitor { .. } => BuiltinActionKind::SendContainerToMonitor,
            Self::CycleSendContainerToMonitor { .. } => {
                BuiltinActionKind::CycleSendContainerToMonitor
            }
            Self::MoveContainerToMonitorWorkspace { .. } => {
                BuiltinActionKind::MoveContainerToMonitorWorkspace
            }
            Self::SendContainerToMonitorWorkspace { .. } => {
                BuiltinActionKind::SendContainerToMonitorWorkspace
            }
            Self::MoveWorkspaceToMonitor { .. } => BuiltinActionKind::MoveWorkspaceToMonitor,
            Self::CycleMoveWorkspaceToMonitor { .. } => {
                BuiltinActionKind::CycleMoveWorkspaceToMonitor
            }
            Self::SwapWorkspacesToMonitor { .. } => BuiltinActionKind::SwapWorkspacesToMonitor,
            Self::PreselectDirection { .. } => BuiltinActionKind::PreselectDirection,
            Self::CancelPreselect => BuiltinActionKind::CancelPreselect,
            Self::Retile => BuiltinActionKind::Retile,
            Self::RetileWithResizeDimensions => BuiltinActionKind::RetileWithResizeDimensions,
            Self::ManageFocusedWindow => BuiltinActionKind::ManageFocusedWindow,
            Self::UnmanageFocusedWindow => BuiltinActionKind::UnmanageFocusedWindow,
            Self::AdjustContainerPadding { .. } => BuiltinActionKind::AdjustContainerPadding,
            Self::AdjustWorkspacePadding { .. } => BuiltinActionKind::AdjustWorkspacePadding,
            Self::ToggleMouseFollowsFocus => BuiltinActionKind::ToggleMouseFollowsFocus,
            Self::SetMouseFollowsFocus { .. } => BuiltinActionKind::SetMouseFollowsFocus,
            Self::ToggleWindowContainerBehaviour => {
                BuiltinActionKind::ToggleWindowContainerBehaviour
            }
            Self::ToggleFloatOverride => BuiltinActionKind::ToggleFloatOverride,
            Self::ToggleWorkspaceWindowContainerBehaviour => {
                BuiltinActionKind::ToggleWorkspaceWindowContainerBehaviour
            }
            Self::ToggleWorkspaceFloatOverride => BuiltinActionKind::ToggleWorkspaceFloatOverride,
            Self::ToggleCrossMonitorMoveBehaviour => {
                BuiltinActionKind::ToggleCrossMonitorMoveBehaviour
            }
            Self::ToggleMonocleFocusBehaviour => BuiltinActionKind::ToggleMonocleFocusBehaviour,
            Self::TogglePause => BuiltinActionKind::TogglePause,
            Self::SetFocusedContainerPadding { .. } => {
                BuiltinActionKind::SetFocusedContainerPadding
            }
            Self::SetFocusedWorkspacePadding { .. } => {
                BuiltinActionKind::SetFocusedWorkspacePadding
            }
            Self::SetContainerPadding { .. } => BuiltinActionKind::SetContainerPadding,
            Self::SetWorkspacePadding { .. } => BuiltinActionKind::SetWorkspacePadding,
            Self::SetWorkspaceTiling { .. } => BuiltinActionKind::SetWorkspaceTiling,
            Self::SetMonitorWorkspaceLayout { .. } => BuiltinActionKind::SetMonitorWorkspaceLayout,
            Self::EnsureWorkspaces { .. } => BuiltinActionKind::EnsureWorkspaces,
            Self::ClearWorkspaceLayoutRules { .. } => BuiltinActionKind::ClearWorkspaceLayoutRules,
            Self::SetScrollingColumns { .. } => BuiltinActionKind::SetScrollingColumns,
            Self::LockContainer { .. } => BuiltinActionKind::LockContainer,
            Self::UnlockContainer { .. } => BuiltinActionKind::UnlockContainer,
            Self::ToggleTitleBars => BuiltinActionKind::ToggleTitleBars,
            Self::EnforceWorkspaceRules => BuiltinActionKind::EnforceWorkspaceRules,
            Self::AddSessionFloatRule => BuiltinActionKind::AddSessionFloatRule,
            Self::ClearSessionFloatRules => BuiltinActionKind::ClearSessionFloatRules,
            Self::ResizeWindowEdge { .. } => BuiltinActionKind::ResizeWindowEdge,
            Self::ResizeWindowEdgeByStep { .. } => BuiltinActionKind::ResizeWindowEdgeByStep,
            Self::SetWindowHidingBehaviour { .. } => BuiltinActionKind::SetWindowHidingBehaviour,
            Self::SetCrossMonitorMoveBehaviour { .. } => {
                BuiltinActionKind::SetCrossMonitorMoveBehaviour
            }
            Self::SetMonocleFocusBehaviour { .. } => BuiltinActionKind::SetMonocleFocusBehaviour,
            Self::SetUnmanagedWindowOperationBehaviour { .. } => {
                BuiltinActionKind::SetUnmanagedWindowOperationBehaviour
            }
            Self::SetFocusFollowsMouse { .. } => BuiltinActionKind::SetFocusFollowsMouse,
            Self::ToggleFocusFollowsMouse { .. } => BuiltinActionKind::ToggleFocusFollowsMouse,
            Self::AddWorkspaceLayoutRule { .. } => BuiltinActionKind::AddWorkspaceLayoutRule,
            Self::FocusNamedWorkspace { .. } => BuiltinActionKind::FocusNamedWorkspace,
            Self::MoveContainerToNamedWorkspace { .. } => {
                BuiltinActionKind::MoveContainerToNamedWorkspace
            }
            Self::SendContainerToNamedWorkspace { .. } => {
                BuiltinActionKind::SendContainerToNamedWorkspace
            }
            Self::SetNamedWorkspaceContainerPadding { .. } => {
                BuiltinActionKind::SetNamedWorkspaceContainerPadding
            }
            Self::SetNamedWorkspacePadding { .. } => BuiltinActionKind::SetNamedWorkspacePadding,
            Self::SetNamedWorkspaceTiling { .. } => BuiltinActionKind::SetNamedWorkspaceTiling,
            Self::SetNamedWorkspaceLayout { .. } => BuiltinActionKind::SetNamedWorkspaceLayout,
            Self::SetNamedWorkspaceCustomLayout { .. } => {
                BuiltinActionKind::SetNamedWorkspaceCustomLayout
            }
            Self::AddNamedWorkspaceLayoutRule { .. } => {
                BuiltinActionKind::AddNamedWorkspaceLayoutRule
            }
            Self::AddNamedWorkspaceCustomLayoutRule { .. } => {
                BuiltinActionKind::AddNamedWorkspaceCustomLayoutRule
            }
            Self::ClearNamedWorkspaceLayoutRules { .. } => {
                BuiltinActionKind::ClearNamedWorkspaceLayoutRules
            }
            Self::EnsureNamedWorkspaces { .. } => BuiltinActionKind::EnsureNamedWorkspaces,
            Self::SetWorkspaceName { .. } => BuiltinActionKind::SetWorkspaceName,
            Self::SetLayoutRatios { .. } => BuiltinActionKind::SetLayoutRatios,
            Self::SetCustomLayout { .. } => BuiltinActionKind::SetCustomLayout,
            Self::SetWorkspaceCustomLayout { .. } => BuiltinActionKind::SetWorkspaceCustomLayout,
            Self::AddWorkspaceCustomLayoutRule { .. } => {
                BuiltinActionKind::AddWorkspaceCustomLayoutRule
            }
            Self::EagerFocus { .. } => BuiltinActionKind::EagerFocus,
            Self::RemoveTitleBar { .. } => BuiltinActionKind::RemoveTitleBar,
            Self::SetResizeStep { .. } => BuiltinActionKind::SetResizeStep,
            Self::SetTransparencyEnabled { .. } => BuiltinActionKind::SetTransparencyEnabled,
            Self::ToggleTransparency => BuiltinActionKind::ToggleTransparency,
            Self::SetTransparencyAlpha { .. } => BuiltinActionKind::SetTransparencyAlpha,
            Self::SetBorderEnabled { .. } => BuiltinActionKind::SetBorderEnabled,
            Self::SetBorderColour { .. } => BuiltinActionKind::SetBorderColour,
            Self::SetBorderWidth { .. } => BuiltinActionKind::SetBorderWidth,
            Self::SetBorderOffset { .. } => BuiltinActionKind::SetBorderOffset,
            Self::SetBorderStyle { .. } => BuiltinActionKind::SetBorderStyle,
            Self::SetBorderImplementation { .. } => BuiltinActionKind::SetBorderImplementation,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_has_one_stable_id() {
        let mut ids: Vec<_> = BuiltinActionKind::ALL
            .iter()
            .map(|kind| kind.id().as_str())
            .collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), BuiltinActionKind::ALL.len());
        let protocol_ids = komorebi_protocol::BuiltInActionId::ALL
            .iter()
            .map(|action| action.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            ids.into_iter().collect::<std::collections::BTreeSet<_>>(),
            protocol_ids
        );
    }

    #[test]
    fn pixel_delta_rejects_zero() {
        assert_eq!(Pixels::new(0), Err(PixelsError::Zero));
        assert_eq!(Pixels::new(24).unwrap().get(), 24);
        assert_eq!(Pixels::new(-8).unwrap().get(), -8);
    }

    #[test]
    fn bound_action_round_trips_through_json() {
        let action = BuiltinAction::ResizeWindow {
            axis: Axis::Horizontal,
            delta: Pixels::new(16).unwrap(),
        };
        let encoded = serde_json::to_string(&action).unwrap();
        let decoded: BuiltinAction = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, action);
        assert_eq!(decoded.kind(), BuiltinActionKind::ResizeWindow);
    }

    #[test]
    fn workspace_name_rejects_empty() {
        assert_eq!(WorkspaceName::parse(""), Err(WorkspaceNameError::Empty));
        assert_eq!(WorkspaceName::parse("   "), Err(WorkspaceNameError::Empty));
        assert_eq!(WorkspaceName::parse("chat").unwrap().as_str(), "chat");
    }
}
