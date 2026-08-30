use super::builtin::BuiltinAction;
use super::offer::ActionSnapshot;
use super::offer::Unavailability;
use super::offer::neighbor_in;
use super::outcome::ActionResult;
use super::outcome::NativeEffect;

pub(super) fn resolve_contextual_inputs(
    snapshot: &ActionSnapshot,
    action: &BuiltinAction,
) -> Result<BuiltinAction, Unavailability> {
    match action {
        BuiltinAction::ResizeWindowByStep { axis, sizing } => Ok(BuiltinAction::ResizeWindow {
            axis: *axis,
            delta: super::builtin::Pixels::from_resize_step(
                snapshot.configuration.resize_step,
                *sizing,
            ),
        }),
        BuiltinAction::ResizeWindowEdgeByStep { direction, sizing } => {
            Ok(BuiltinAction::ResizeWindowEdge {
                direction: *direction,
                delta: super::builtin::Pixels::from_resize_step(
                    snapshot.configuration.resize_step,
                    *sizing,
                ),
            })
        }
        BuiltinAction::FocusNamedWorkspace { name } => {
            let (monitor, workspace) = snapshot
                .workspace_by_name(name)
                .ok_or(Unavailability::UnknownWorkspace)?;
            Ok(BuiltinAction::FocusMonitorWorkspace { monitor, workspace })
        }
        BuiltinAction::MoveContainerToNamedWorkspace { name } => {
            let (monitor, workspace) = snapshot
                .workspace_by_name(name)
                .ok_or(Unavailability::UnknownWorkspace)?;
            Ok(BuiltinAction::MoveContainerToMonitorWorkspace { monitor, workspace })
        }
        BuiltinAction::SendContainerToNamedWorkspace { name } => {
            let (monitor, workspace) = snapshot
                .workspace_by_name(name)
                .ok_or(Unavailability::UnknownWorkspace)?;
            Ok(BuiltinAction::SendContainerToMonitorWorkspace { monitor, workspace })
        }
        BuiltinAction::SetNamedWorkspaceContainerPadding { name, size } => {
            let (monitor, workspace) = snapshot
                .workspace_by_name(name)
                .ok_or(Unavailability::UnknownWorkspace)?;
            Ok(BuiltinAction::SetContainerPadding {
                monitor,
                workspace,
                size: *size,
            })
        }
        BuiltinAction::SetNamedWorkspacePadding { name, size } => {
            let (monitor, workspace) = snapshot
                .workspace_by_name(name)
                .ok_or(Unavailability::UnknownWorkspace)?;
            Ok(BuiltinAction::SetWorkspacePadding {
                monitor,
                workspace,
                size: *size,
            })
        }
        BuiltinAction::SetNamedWorkspaceTiling { name, tile } => {
            let (monitor, workspace) = snapshot
                .workspace_by_name(name)
                .ok_or(Unavailability::UnknownWorkspace)?;
            Ok(BuiltinAction::SetWorkspaceTiling {
                monitor,
                workspace,
                tile: *tile,
            })
        }
        BuiltinAction::SetNamedWorkspaceLayout { name, layout } => {
            let (monitor, workspace) = snapshot
                .workspace_by_name(name)
                .ok_or(Unavailability::UnknownWorkspace)?;
            Ok(BuiltinAction::SetMonitorWorkspaceLayout {
                monitor,
                workspace,
                layout: *layout,
            })
        }
        BuiltinAction::SetNamedWorkspaceCustomLayout { name, path } => {
            let (monitor, workspace) = snapshot
                .workspace_by_name(name)
                .ok_or(Unavailability::UnknownWorkspace)?;
            Ok(BuiltinAction::SetWorkspaceCustomLayout {
                monitor,
                workspace,
                path: path.clone(),
            })
        }
        BuiltinAction::AddNamedWorkspaceLayoutRule {
            name,
            at_container_count,
            layout,
        } => {
            let (monitor, workspace) = snapshot
                .workspace_by_name(name)
                .ok_or(Unavailability::UnknownWorkspace)?;
            Ok(BuiltinAction::AddWorkspaceLayoutRule {
                monitor,
                workspace,
                at_container_count: *at_container_count,
                layout: *layout,
            })
        }
        BuiltinAction::AddNamedWorkspaceCustomLayoutRule {
            name,
            at_container_count,
            path,
        } => {
            let (monitor, workspace) = snapshot
                .workspace_by_name(name)
                .ok_or(Unavailability::UnknownWorkspace)?;
            Ok(BuiltinAction::AddWorkspaceCustomLayoutRule {
                monitor,
                workspace,
                at_container_count: *at_container_count,
                path: path.clone(),
            })
        }
        BuiltinAction::ClearNamedWorkspaceLayoutRules { name } => {
            let (monitor, workspace) = snapshot
                .workspace_by_name(name)
                .ok_or(Unavailability::UnknownWorkspace)?;
            Ok(BuiltinAction::ClearWorkspaceLayoutRules { monitor, workspace })
        }
        other => Ok(other.clone()),
    }
}

pub(super) fn directional_gap(
    snapshot: &ActionSnapshot,
    action: &BuiltinAction,
) -> Option<Unavailability> {
    match action.clone() {
        BuiltinAction::FocusWindow { direction }
        | BuiltinAction::MoveWindow { direction }
        | BuiltinAction::PromoteWindow { direction } => {
            if neighbor_in(snapshot, direction) {
                None
            } else {
                Some(Unavailability::NoWindowInDirection)
            }
        }
        BuiltinAction::ResizeWindow { .. }
        | BuiltinAction::ResizeWindowByStep { .. }
        | BuiltinAction::SetResizeStep { .. }
        | BuiltinAction::SetTransparencyEnabled { .. }
        | BuiltinAction::ToggleTransparency
        | BuiltinAction::SetTransparencyAlpha { .. }
        | BuiltinAction::SetBorderEnabled { .. }
        | BuiltinAction::SetBorderColour { .. }
        | BuiltinAction::SetBorderWidth { .. }
        | BuiltinAction::SetBorderOffset { .. }
        | BuiltinAction::SetBorderStyle { .. }
        | BuiltinAction::SetBorderImplementation { .. }
        | BuiltinAction::SetWorkspaceLayout { .. }
        | BuiltinAction::ToggleWindowFloat { .. }
        | BuiltinAction::CycleFocusWindow { .. }
        | BuiltinAction::CycleMoveWindow { .. }
        | BuiltinAction::ToggleWindowMonocle { .. }
        | BuiltinAction::ToggleWindowMaximize { .. }
        | BuiltinAction::ToggleContainerLock { .. }
        | BuiltinAction::StackWindow { .. }
        | BuiltinAction::UnstackWindow { .. }
        | BuiltinAction::StackAll
        | BuiltinAction::UnstackAll
        | BuiltinAction::CycleStack { .. }
        | BuiltinAction::CycleStackIndex { .. }
        | BuiltinAction::FocusStackWindow { .. }
        | BuiltinAction::FocusWorkspace { .. }
        | BuiltinAction::CycleFocusWorkspace { .. }
        | BuiltinAction::CycleFocusEmptyWorkspace { .. }
        | BuiltinAction::FocusLastWorkspace
        | BuiltinAction::CloseWorkspace
        | BuiltinAction::FocusMonitor { .. }
        | BuiltinAction::CycleFocusMonitor { .. }
        | BuiltinAction::FocusMonitorAtCursor
        | BuiltinAction::FocusWorkspaceOnAllMonitors { .. }
        | BuiltinAction::FocusMonitorWorkspace { .. }
        | BuiltinAction::CloseWindow { .. }
        | BuiltinAction::MinimizeWindow { .. }
        | BuiltinAction::ForceFocus { .. }
        | BuiltinAction::PromoteContainer
        | BuiltinAction::PromoteContainerSwap
        | BuiltinAction::PromoteFocus
        | BuiltinAction::NewWorkspace
        | BuiltinAction::ToggleTiling
        | BuiltinAction::CycleLayout { .. }
        | BuiltinAction::FlipLayout { .. }
        | BuiltinAction::ToggleWorkspaceLayer
        | BuiltinAction::MoveContainerToLastWorkspace
        | BuiltinAction::SendContainerToLastWorkspace
        | BuiltinAction::MoveContainerToWorkspace { .. }
        | BuiltinAction::CycleMoveContainerToWorkspace { .. }
        | BuiltinAction::SendContainerToWorkspace { .. }
        | BuiltinAction::CycleSendContainerToWorkspace { .. }
        | BuiltinAction::MoveContainerToMonitor { .. }
        | BuiltinAction::CycleMoveContainerToMonitor { .. }
        | BuiltinAction::SendContainerToMonitor { .. }
        | BuiltinAction::CycleSendContainerToMonitor { .. }
        | BuiltinAction::MoveContainerToMonitorWorkspace { .. }
        | BuiltinAction::SendContainerToMonitorWorkspace { .. }
        | BuiltinAction::MoveWorkspaceToMonitor { .. }
        | BuiltinAction::CycleMoveWorkspaceToMonitor { .. }
        | BuiltinAction::SwapWorkspacesToMonitor { .. }
        | BuiltinAction::PreselectDirection { .. }
        | BuiltinAction::CancelPreselect
        | BuiltinAction::Retile
        | BuiltinAction::RetileWithResizeDimensions
        | BuiltinAction::ManageFocusedWindow
        | BuiltinAction::UnmanageFocusedWindow
        | BuiltinAction::AdjustContainerPadding { .. }
        | BuiltinAction::AdjustWorkspacePadding { .. }
        | BuiltinAction::ToggleMouseFollowsFocus
        | BuiltinAction::SetMouseFollowsFocus { .. }
        | BuiltinAction::ToggleWindowContainerBehaviour
        | BuiltinAction::ToggleFloatOverride
        | BuiltinAction::ToggleWorkspaceWindowContainerBehaviour
        | BuiltinAction::ToggleWorkspaceFloatOverride
        | BuiltinAction::ToggleCrossMonitorMoveBehaviour
        | BuiltinAction::ToggleMonocleFocusBehaviour
        | BuiltinAction::TogglePause
        | BuiltinAction::SetFocusedContainerPadding { .. }
        | BuiltinAction::SetFocusedWorkspacePadding { .. }
        | BuiltinAction::SetContainerPadding { .. }
        | BuiltinAction::SetWorkspacePadding { .. }
        | BuiltinAction::SetWorkspaceTiling { .. }
        | BuiltinAction::SetMonitorWorkspaceLayout { .. }
        | BuiltinAction::EnsureWorkspaces { .. }
        | BuiltinAction::ClearWorkspaceLayoutRules { .. }
        | BuiltinAction::SetScrollingColumns { .. }
        | BuiltinAction::LockContainer { .. }
        | BuiltinAction::UnlockContainer { .. }
        | BuiltinAction::ToggleTitleBars
        | BuiltinAction::EnforceWorkspaceRules
        | BuiltinAction::AddSessionFloatRule
        | BuiltinAction::ClearSessionFloatRules
        | BuiltinAction::ResizeWindowEdge { .. }
        | BuiltinAction::ResizeWindowEdgeByStep { .. }
        | BuiltinAction::SetWindowHidingBehaviour { .. }
        | BuiltinAction::SetCrossMonitorMoveBehaviour { .. }
        | BuiltinAction::SetMonocleFocusBehaviour { .. }
        | BuiltinAction::SetUnmanagedWindowOperationBehaviour { .. }
        | BuiltinAction::SetFocusFollowsMouse { .. }
        | BuiltinAction::ToggleFocusFollowsMouse { .. }
        | BuiltinAction::AddWorkspaceLayoutRule { .. }
        | BuiltinAction::FocusNamedWorkspace { .. }
        | BuiltinAction::MoveContainerToNamedWorkspace { .. }
        | BuiltinAction::SendContainerToNamedWorkspace { .. }
        | BuiltinAction::SetNamedWorkspaceContainerPadding { .. }
        | BuiltinAction::SetNamedWorkspacePadding { .. }
        | BuiltinAction::SetNamedWorkspaceTiling { .. }
        | BuiltinAction::SetNamedWorkspaceLayout { .. }
        | BuiltinAction::SetNamedWorkspaceCustomLayout { .. }
        | BuiltinAction::AddNamedWorkspaceLayoutRule { .. }
        | BuiltinAction::AddNamedWorkspaceCustomLayoutRule { .. }
        | BuiltinAction::ClearNamedWorkspaceLayoutRules { .. }
        | BuiltinAction::EnsureNamedWorkspaces { .. }
        | BuiltinAction::SetWorkspaceName { .. }
        | BuiltinAction::SetLayoutRatios { .. }
        | BuiltinAction::SetCustomLayout { .. }
        | BuiltinAction::SetWorkspaceCustomLayout { .. }
        | BuiltinAction::AddWorkspaceCustomLayoutRule { .. }
        | BuiltinAction::EagerFocus { .. }
        | BuiltinAction::RemoveTitleBar { .. } => None,
    }
}

pub(super) fn apply_logical(snapshot: &mut ActionSnapshot, action: &BuiltinAction) {
    match action.clone() {
        BuiltinAction::SetWorkspaceLayout { layout, .. } => snapshot.current_layout = layout,
        BuiltinAction::SetResizeStep { step } => snapshot.configuration.resize_step = step,
        BuiltinAction::SetTransparencyEnabled { enabled } => {
            snapshot.configuration.transparency.enabled = enabled;
        }
        BuiltinAction::ToggleTransparency => {
            snapshot.configuration.transparency.enabled =
                !snapshot.configuration.transparency.enabled;
        }
        BuiltinAction::SetTransparencyAlpha { alpha } => {
            snapshot.configuration.transparency.alpha = alpha;
        }
        BuiltinAction::SetBorderEnabled { enabled } => {
            snapshot.configuration.border.enabled = enabled;
        }
        BuiltinAction::SetBorderWidth { width } => {
            snapshot.configuration.border.width = width;
        }
        BuiltinAction::SetBorderOffset { offset } => {
            snapshot.configuration.border.offset = offset;
        }
        BuiltinAction::SetBorderStyle { style } => {
            snapshot.configuration.border.style = style;
        }
        BuiltinAction::SetBorderImplementation { implementation } => {
            snapshot.configuration.border.implementation = implementation;
        }
        BuiltinAction::ToggleWindowFloat { .. } => {
            snapshot.focused_window_floating = !snapshot.focused_window_floating;
        }
        BuiltinAction::TogglePause => {
            snapshot.paused = !snapshot.paused;
        }
        BuiltinAction::SetBorderColour { .. }
        | BuiltinAction::FocusWindow { .. }
        | BuiltinAction::MoveWindow { .. }
        | BuiltinAction::ResizeWindow { .. }
        | BuiltinAction::ResizeWindowByStep { .. }
        | BuiltinAction::CycleFocusWindow { .. }
        | BuiltinAction::CycleMoveWindow { .. }
        | BuiltinAction::ToggleWindowMonocle { .. }
        | BuiltinAction::ToggleWindowMaximize { .. }
        | BuiltinAction::ToggleContainerLock { .. }
        | BuiltinAction::StackWindow { .. }
        | BuiltinAction::UnstackWindow { .. }
        | BuiltinAction::StackAll
        | BuiltinAction::UnstackAll
        | BuiltinAction::CycleStack { .. }
        | BuiltinAction::CycleStackIndex { .. }
        | BuiltinAction::FocusStackWindow { .. }
        | BuiltinAction::FocusWorkspace { .. }
        | BuiltinAction::CycleFocusWorkspace { .. }
        | BuiltinAction::CycleFocusEmptyWorkspace { .. }
        | BuiltinAction::FocusLastWorkspace
        | BuiltinAction::CloseWorkspace
        | BuiltinAction::FocusMonitor { .. }
        | BuiltinAction::CycleFocusMonitor { .. }
        | BuiltinAction::FocusMonitorAtCursor
        | BuiltinAction::FocusWorkspaceOnAllMonitors { .. }
        | BuiltinAction::FocusMonitorWorkspace { .. }
        | BuiltinAction::CloseWindow { .. }
        | BuiltinAction::MinimizeWindow { .. }
        | BuiltinAction::ForceFocus { .. }
        | BuiltinAction::PromoteContainer
        | BuiltinAction::PromoteContainerSwap
        | BuiltinAction::PromoteFocus
        | BuiltinAction::PromoteWindow { .. }
        | BuiltinAction::NewWorkspace
        | BuiltinAction::ToggleTiling
        | BuiltinAction::CycleLayout { .. }
        | BuiltinAction::FlipLayout { .. }
        | BuiltinAction::ToggleWorkspaceLayer
        | BuiltinAction::MoveContainerToLastWorkspace
        | BuiltinAction::SendContainerToLastWorkspace
        | BuiltinAction::MoveContainerToWorkspace { .. }
        | BuiltinAction::CycleMoveContainerToWorkspace { .. }
        | BuiltinAction::SendContainerToWorkspace { .. }
        | BuiltinAction::CycleSendContainerToWorkspace { .. }
        | BuiltinAction::MoveContainerToMonitor { .. }
        | BuiltinAction::CycleMoveContainerToMonitor { .. }
        | BuiltinAction::SendContainerToMonitor { .. }
        | BuiltinAction::CycleSendContainerToMonitor { .. }
        | BuiltinAction::MoveContainerToMonitorWorkspace { .. }
        | BuiltinAction::SendContainerToMonitorWorkspace { .. }
        | BuiltinAction::MoveWorkspaceToMonitor { .. }
        | BuiltinAction::CycleMoveWorkspaceToMonitor { .. }
        | BuiltinAction::SwapWorkspacesToMonitor { .. }
        | BuiltinAction::PreselectDirection { .. }
        | BuiltinAction::CancelPreselect
        | BuiltinAction::Retile
        | BuiltinAction::RetileWithResizeDimensions
        | BuiltinAction::ManageFocusedWindow
        | BuiltinAction::UnmanageFocusedWindow
        | BuiltinAction::AdjustContainerPadding { .. }
        | BuiltinAction::AdjustWorkspacePadding { .. }
        | BuiltinAction::ToggleMouseFollowsFocus
        | BuiltinAction::SetMouseFollowsFocus { .. }
        | BuiltinAction::ToggleWindowContainerBehaviour
        | BuiltinAction::ToggleFloatOverride
        | BuiltinAction::ToggleWorkspaceWindowContainerBehaviour
        | BuiltinAction::ToggleWorkspaceFloatOverride
        | BuiltinAction::ToggleCrossMonitorMoveBehaviour
        | BuiltinAction::ToggleMonocleFocusBehaviour
        | BuiltinAction::SetFocusedContainerPadding { .. }
        | BuiltinAction::SetFocusedWorkspacePadding { .. }
        | BuiltinAction::SetContainerPadding { .. }
        | BuiltinAction::SetWorkspacePadding { .. }
        | BuiltinAction::SetWorkspaceTiling { .. }
        | BuiltinAction::SetMonitorWorkspaceLayout { .. }
        | BuiltinAction::EnsureWorkspaces { .. }
        | BuiltinAction::ClearWorkspaceLayoutRules { .. }
        | BuiltinAction::SetScrollingColumns { .. }
        | BuiltinAction::LockContainer { .. }
        | BuiltinAction::UnlockContainer { .. }
        | BuiltinAction::ToggleTitleBars
        | BuiltinAction::EnforceWorkspaceRules
        | BuiltinAction::AddSessionFloatRule
        | BuiltinAction::ClearSessionFloatRules
        | BuiltinAction::ResizeWindowEdge { .. }
        | BuiltinAction::ResizeWindowEdgeByStep { .. }
        | BuiltinAction::SetWindowHidingBehaviour { .. }
        | BuiltinAction::SetCrossMonitorMoveBehaviour { .. }
        | BuiltinAction::SetMonocleFocusBehaviour { .. }
        | BuiltinAction::SetUnmanagedWindowOperationBehaviour { .. }
        | BuiltinAction::SetFocusFollowsMouse { .. }
        | BuiltinAction::ToggleFocusFollowsMouse { .. }
        | BuiltinAction::AddWorkspaceLayoutRule { .. }
        | BuiltinAction::FocusNamedWorkspace { .. }
        | BuiltinAction::MoveContainerToNamedWorkspace { .. }
        | BuiltinAction::SendContainerToNamedWorkspace { .. }
        | BuiltinAction::SetNamedWorkspaceContainerPadding { .. }
        | BuiltinAction::SetNamedWorkspacePadding { .. }
        | BuiltinAction::SetNamedWorkspaceTiling { .. }
        | BuiltinAction::SetNamedWorkspaceLayout { .. }
        | BuiltinAction::SetNamedWorkspaceCustomLayout { .. }
        | BuiltinAction::AddNamedWorkspaceLayoutRule { .. }
        | BuiltinAction::AddNamedWorkspaceCustomLayoutRule { .. }
        | BuiltinAction::ClearNamedWorkspaceLayoutRules { .. }
        | BuiltinAction::EnsureNamedWorkspaces { .. }
        | BuiltinAction::SetWorkspaceName { .. }
        | BuiltinAction::SetLayoutRatios { .. }
        | BuiltinAction::SetCustomLayout { .. }
        | BuiltinAction::SetWorkspaceCustomLayout { .. }
        | BuiltinAction::AddWorkspaceCustomLayoutRule { .. }
        | BuiltinAction::EagerFocus { .. }
        | BuiltinAction::RemoveTitleBar { .. } => {}
    }
}

pub(super) fn logical_result(action: &BuiltinAction, snapshot: &ActionSnapshot) -> ActionResult {
    match action.clone() {
        BuiltinAction::FocusWindow { direction } => ActionResult::Focused { direction },
        BuiltinAction::MoveWindow { direction } => ActionResult::Moved { direction },
        BuiltinAction::ResizeWindow { axis, delta } => ActionResult::Resized { axis, delta },
        BuiltinAction::SetResizeStep { step } => ActionResult::ResizeStepSet { step },
        BuiltinAction::SetTransparencyEnabled { enabled } => {
            ActionResult::TransparencyEnabledSet { enabled }
        }
        BuiltinAction::ToggleTransparency => ActionResult::TransparencyToggled {
            enabled: snapshot.configuration.transparency.enabled,
        },
        BuiltinAction::SetTransparencyAlpha { alpha } => {
            ActionResult::TransparencyAlphaSet { alpha }
        }
        BuiltinAction::SetBorderEnabled { enabled } => ActionResult::BorderEnabledSet { enabled },
        BuiltinAction::SetBorderColour {
            window_kind,
            colour,
        } => ActionResult::BorderColourSet {
            window_kind,
            colour,
        },
        BuiltinAction::SetBorderWidth { width } => ActionResult::BorderWidthSet { width },
        BuiltinAction::SetBorderOffset { offset } => ActionResult::BorderOffsetSet { offset },
        BuiltinAction::SetBorderStyle { style } => ActionResult::BorderStyleSet { style },
        BuiltinAction::SetBorderImplementation { implementation } => {
            ActionResult::BorderImplementationSet { implementation }
        }
        BuiltinAction::SetWorkspaceLayout { layout, .. } => ActionResult::LayoutSet { layout },
        BuiltinAction::ToggleWindowFloat { .. } => ActionResult::FloatToggled {
            floating: snapshot.focused_window_floating,
        },
        BuiltinAction::CycleFocusWindow { direction } => ActionResult::CycleFocused { direction },
        BuiltinAction::CycleMoveWindow { direction } => ActionResult::CycleMoved { direction },
        BuiltinAction::ToggleWindowMonocle { .. } => ActionResult::MonocleToggled,
        BuiltinAction::ToggleWindowMaximize { .. } => ActionResult::MaximizeToggled,
        BuiltinAction::ToggleContainerLock { .. } => ActionResult::LockToggled,
        BuiltinAction::StackWindow { direction } => ActionResult::Stacked { direction },
        BuiltinAction::UnstackWindow { .. } => ActionResult::Unstacked,
        BuiltinAction::StackAll => ActionResult::StackedAll,
        BuiltinAction::UnstackAll => ActionResult::UnstackedAll,
        BuiltinAction::CycleStack { direction } => ActionResult::StackCycled { direction },
        BuiltinAction::CycleStackIndex { direction } => {
            ActionResult::StackIndexCycled { direction }
        }
        BuiltinAction::FocusStackWindow { index } => ActionResult::StackWindowFocused { index },
        BuiltinAction::FocusWorkspace { index } => ActionResult::WorkspaceFocused { index },
        BuiltinAction::CycleFocusWorkspace { direction } => {
            ActionResult::WorkspaceCycled { direction }
        }
        BuiltinAction::CycleFocusEmptyWorkspace { direction } => {
            ActionResult::EmptyWorkspaceCycled { direction }
        }
        BuiltinAction::FocusLastWorkspace => ActionResult::LastWorkspaceFocused,
        BuiltinAction::CloseWorkspace => ActionResult::WorkspaceClosed,
        BuiltinAction::FocusMonitor { index } => ActionResult::MonitorFocused { index },
        BuiltinAction::CycleFocusMonitor { direction } => ActionResult::MonitorCycled { direction },
        BuiltinAction::FocusMonitorAtCursor => ActionResult::MonitorAtCursorFocused,
        BuiltinAction::FocusWorkspaceOnAllMonitors { index } => {
            ActionResult::WorkspaceFocusedOnAllMonitors { index }
        }
        BuiltinAction::FocusMonitorWorkspace { monitor, workspace } => {
            ActionResult::MonitorWorkspaceFocused { monitor, workspace }
        }
        BuiltinAction::CloseWindow { .. } => ActionResult::WindowClosed,
        BuiltinAction::MinimizeWindow { .. } => ActionResult::WindowMinimized,
        BuiltinAction::ForceFocus { .. } => ActionResult::FocusForced,
        BuiltinAction::PromoteContainer => ActionResult::ContainerPromoted,
        BuiltinAction::PromoteContainerSwap => ActionResult::ContainerPromoteSwapped,
        BuiltinAction::PromoteFocus => ActionResult::FocusPromoted,
        BuiltinAction::PromoteWindow { direction } => ActionResult::WindowPromoted { direction },
        BuiltinAction::NewWorkspace => ActionResult::WorkspaceCreated,
        BuiltinAction::ToggleTiling => ActionResult::TilingToggled,
        BuiltinAction::CycleLayout { direction } => ActionResult::LayoutCycled { direction },
        BuiltinAction::FlipLayout { axis } => ActionResult::LayoutFlipped { axis },
        BuiltinAction::ToggleWorkspaceLayer => ActionResult::WorkspaceLayerToggled,
        BuiltinAction::MoveContainerToLastWorkspace => ActionResult::ContainerMovedToLastWorkspace,
        BuiltinAction::SendContainerToLastWorkspace => ActionResult::ContainerSentToLastWorkspace,
        BuiltinAction::MoveContainerToWorkspace { index } => {
            ActionResult::ContainerMovedToWorkspace { index }
        }
        BuiltinAction::CycleMoveContainerToWorkspace { direction } => {
            ActionResult::ContainerCycledToWorkspace { direction }
        }
        BuiltinAction::SendContainerToWorkspace { index } => {
            ActionResult::ContainerSentToWorkspace { index }
        }
        BuiltinAction::CycleSendContainerToWorkspace { direction } => {
            ActionResult::ContainerCycleSentToWorkspace { direction }
        }
        BuiltinAction::MoveContainerToMonitor { index } => {
            ActionResult::ContainerMovedToMonitor { index }
        }
        BuiltinAction::CycleMoveContainerToMonitor { direction } => {
            ActionResult::ContainerCycledToMonitor { direction }
        }
        BuiltinAction::SendContainerToMonitor { index } => {
            ActionResult::ContainerSentToMonitor { index }
        }
        BuiltinAction::CycleSendContainerToMonitor { direction } => {
            ActionResult::ContainerCycleSentToMonitor { direction }
        }
        BuiltinAction::MoveContainerToMonitorWorkspace { monitor, workspace } => {
            ActionResult::ContainerMovedToMonitorWorkspace { monitor, workspace }
        }
        BuiltinAction::SendContainerToMonitorWorkspace { monitor, workspace } => {
            ActionResult::ContainerSentToMonitorWorkspace { monitor, workspace }
        }
        BuiltinAction::MoveWorkspaceToMonitor { index } => {
            ActionResult::WorkspaceMovedToMonitor { index }
        }
        BuiltinAction::CycleMoveWorkspaceToMonitor { direction } => {
            ActionResult::WorkspaceCycledToMonitor { direction }
        }
        BuiltinAction::SwapWorkspacesToMonitor { index } => {
            ActionResult::WorkspacesSwappedToMonitor { index }
        }
        BuiltinAction::PreselectDirection { direction } => {
            ActionResult::DirectionPreselected { direction }
        }
        BuiltinAction::CancelPreselect => ActionResult::PreselectCancelled,
        BuiltinAction::Retile => ActionResult::Retiled,
        BuiltinAction::RetileWithResizeDimensions => ActionResult::RetiledWithResizeDimensions,
        BuiltinAction::ManageFocusedWindow => ActionResult::FocusedWindowManaged,
        BuiltinAction::UnmanageFocusedWindow => ActionResult::FocusedWindowUnmanaged,
        BuiltinAction::AdjustContainerPadding { sizing, adjustment } => {
            ActionResult::ContainerPaddingAdjusted { sizing, adjustment }
        }
        BuiltinAction::AdjustWorkspacePadding { sizing, adjustment } => {
            ActionResult::WorkspacePaddingAdjusted { sizing, adjustment }
        }
        BuiltinAction::ToggleMouseFollowsFocus => ActionResult::MouseFollowsFocusToggled,
        BuiltinAction::SetMouseFollowsFocus { enabled } => {
            ActionResult::MouseFollowsFocusSet { enabled }
        }
        BuiltinAction::ToggleWindowContainerBehaviour => {
            ActionResult::WindowContainerBehaviourToggled
        }
        BuiltinAction::ToggleFloatOverride => ActionResult::FloatOverrideToggled,
        BuiltinAction::ToggleWorkspaceWindowContainerBehaviour => {
            ActionResult::WorkspaceWindowContainerBehaviourToggled
        }
        BuiltinAction::ToggleWorkspaceFloatOverride => ActionResult::WorkspaceFloatOverrideToggled,
        BuiltinAction::ToggleCrossMonitorMoveBehaviour => {
            ActionResult::CrossMonitorMoveBehaviourToggled
        }
        BuiltinAction::ToggleMonocleFocusBehaviour => ActionResult::MonocleFocusBehaviourToggled,
        BuiltinAction::TogglePause => ActionResult::PauseToggled {
            paused: snapshot.paused,
        },
        BuiltinAction::SetFocusedContainerPadding { size } => {
            ActionResult::FocusedContainerPaddingSet { size }
        }
        BuiltinAction::SetFocusedWorkspacePadding { size } => {
            ActionResult::FocusedWorkspacePaddingSet { size }
        }
        BuiltinAction::SetContainerPadding {
            monitor,
            workspace,
            size,
        } => ActionResult::ContainerPaddingSet {
            monitor,
            workspace,
            size,
        },
        BuiltinAction::SetWorkspacePadding {
            monitor,
            workspace,
            size,
        } => ActionResult::WorkspacePaddingSet {
            monitor,
            workspace,
            size,
        },
        BuiltinAction::SetWorkspaceTiling {
            monitor,
            workspace,
            tile,
        } => ActionResult::WorkspaceTilingSet {
            monitor,
            workspace,
            tile,
        },
        BuiltinAction::SetMonitorWorkspaceLayout {
            monitor,
            workspace,
            layout,
        } => ActionResult::MonitorWorkspaceLayoutSet {
            monitor,
            workspace,
            layout,
        },
        BuiltinAction::EnsureWorkspaces { monitor, count } => {
            ActionResult::WorkspacesEnsured { monitor, count }
        }
        BuiltinAction::ClearWorkspaceLayoutRules { monitor, workspace } => {
            ActionResult::WorkspaceLayoutRulesCleared { monitor, workspace }
        }
        BuiltinAction::SetScrollingColumns { columns } => {
            ActionResult::ScrollingColumnsSet { columns }
        }
        BuiltinAction::LockContainer {
            monitor,
            workspace,
            container,
        } => ActionResult::ContainerLocked {
            monitor,
            workspace,
            container,
        },
        BuiltinAction::UnlockContainer {
            monitor,
            workspace,
            container,
        } => ActionResult::ContainerUnlocked {
            monitor,
            workspace,
            container,
        },
        BuiltinAction::ToggleTitleBars => ActionResult::TitleBarsToggled,
        BuiltinAction::EnforceWorkspaceRules => ActionResult::WorkspaceRulesEnforced,
        BuiltinAction::AddSessionFloatRule => ActionResult::SessionFloatRuleAdded,
        BuiltinAction::ClearSessionFloatRules => ActionResult::SessionFloatRulesCleared,
        BuiltinAction::ResizeWindowEdge { direction, delta } => {
            ActionResult::WindowEdgeResized { direction, delta }
        }
        BuiltinAction::SetWindowHidingBehaviour { behaviour } => {
            ActionResult::WindowHidingBehaviourSet { behaviour }
        }
        BuiltinAction::SetCrossMonitorMoveBehaviour { behaviour } => {
            ActionResult::CrossMonitorMoveBehaviourSet { behaviour }
        }
        BuiltinAction::SetMonocleFocusBehaviour { behaviour } => {
            ActionResult::MonocleFocusBehaviourSet { behaviour }
        }
        BuiltinAction::SetUnmanagedWindowOperationBehaviour { behaviour } => {
            ActionResult::UnmanagedWindowOperationBehaviourSet { behaviour }
        }
        BuiltinAction::SetFocusFollowsMouse {
            implementation,
            enabled,
        } => ActionResult::FocusFollowsMouseSet {
            implementation,
            enabled,
        },
        BuiltinAction::ToggleFocusFollowsMouse { implementation } => {
            ActionResult::FocusFollowsMouseToggled { implementation }
        }
        BuiltinAction::AddWorkspaceLayoutRule {
            monitor,
            workspace,
            at_container_count,
            layout,
        } => ActionResult::WorkspaceLayoutRuleAdded {
            monitor,
            workspace,
            at_container_count,
            layout,
        },
        BuiltinAction::ResizeWindowByStep { .. }
        | BuiltinAction::ResizeWindowEdgeByStep { .. }
        | BuiltinAction::FocusNamedWorkspace { .. }
        | BuiltinAction::MoveContainerToNamedWorkspace { .. }
        | BuiltinAction::SendContainerToNamedWorkspace { .. }
        | BuiltinAction::SetNamedWorkspaceContainerPadding { .. }
        | BuiltinAction::SetNamedWorkspacePadding { .. }
        | BuiltinAction::SetNamedWorkspaceTiling { .. }
        | BuiltinAction::SetNamedWorkspaceLayout { .. }
        | BuiltinAction::SetNamedWorkspaceCustomLayout { .. }
        | BuiltinAction::AddNamedWorkspaceLayoutRule { .. }
        | BuiltinAction::AddNamedWorkspaceCustomLayoutRule { .. }
        | BuiltinAction::ClearNamedWorkspaceLayoutRules { .. } => {
            unreachable!("contextual actions resolve before logical result")
        }
        BuiltinAction::EnsureNamedWorkspaces { monitor, .. } => {
            ActionResult::NamedWorkspacesEnsured { monitor }
        }
        BuiltinAction::SetWorkspaceName {
            monitor, workspace, ..
        } => ActionResult::WorkspaceNamed { monitor, workspace },
        BuiltinAction::SetLayoutRatios { .. } => ActionResult::LayoutRatiosSet,
        BuiltinAction::SetCustomLayout { .. } => ActionResult::CustomLayoutSet,
        BuiltinAction::SetWorkspaceCustomLayout {
            monitor, workspace, ..
        } => ActionResult::WorkspaceCustomLayoutSet { monitor, workspace },
        BuiltinAction::AddWorkspaceCustomLayoutRule {
            monitor,
            workspace,
            at_container_count,
            ..
        } => ActionResult::WorkspaceCustomLayoutRuleAdded {
            monitor,
            workspace,
            at_container_count,
        },
        BuiltinAction::EagerFocus { .. } => ActionResult::EagerFocused,
        BuiltinAction::RemoveTitleBar { .. } => ActionResult::TitleBarRemoved,
    }
}

pub(super) fn effects(action: &BuiltinAction, snapshot: &ActionSnapshot) -> Vec<NativeEffect> {
    match action.clone() {
        BuiltinAction::FocusWindow { direction } => {
            vec![NativeEffect::FocusNeighbor { direction }]
        }
        BuiltinAction::MoveWindow { direction } => {
            vec![NativeEffect::MoveNeighbor { direction }]
        }
        BuiltinAction::ResizeWindow { axis, delta } => {
            vec![NativeEffect::Resize { axis, delta }]
        }
        BuiltinAction::SetResizeStep { step } => vec![NativeEffect::SetResizeStep { step }],
        BuiltinAction::SetTransparencyEnabled { enabled } => {
            vec![NativeEffect::SetTransparencyEnabled { enabled }]
        }
        BuiltinAction::ToggleTransparency => vec![NativeEffect::SetTransparencyEnabled {
            enabled: snapshot.configuration.transparency.enabled,
        }],
        BuiltinAction::SetTransparencyAlpha { alpha } => {
            vec![NativeEffect::SetTransparencyAlpha { alpha }]
        }
        BuiltinAction::SetBorderEnabled { enabled } => vec![NativeEffect::SetBorderEnabled {
            enabled,
            implementation: snapshot.configuration.border.implementation,
        }],
        BuiltinAction::SetBorderColour {
            window_kind,
            colour,
        } => vec![NativeEffect::SetBorderColour {
            window_kind,
            colour,
        }],
        BuiltinAction::SetBorderWidth { width } => vec![NativeEffect::SetBorderWidth { width }],
        BuiltinAction::SetBorderOffset { offset } => vec![NativeEffect::SetBorderOffset { offset }],
        BuiltinAction::SetBorderStyle { style } => vec![NativeEffect::SetBorderStyle { style }],
        BuiltinAction::SetBorderImplementation { implementation } => {
            vec![NativeEffect::SetBorderImplementation { implementation }]
        }
        BuiltinAction::SetWorkspaceLayout { layout, .. } => {
            vec![NativeEffect::SetLayout { layout }]
        }
        BuiltinAction::ToggleWindowFloat { .. } => {
            vec![NativeEffect::SetWindowFloating {
                floating: snapshot.focused_window_floating,
            }]
        }
        BuiltinAction::CycleFocusWindow { direction } => {
            vec![NativeEffect::CycleFocus { direction }]
        }
        BuiltinAction::CycleMoveWindow { direction } => {
            vec![NativeEffect::CycleMove { direction }]
        }
        BuiltinAction::ToggleWindowMonocle { .. } => vec![NativeEffect::ToggleMonocle],
        BuiltinAction::ToggleWindowMaximize { .. } => vec![NativeEffect::ToggleMaximize],
        BuiltinAction::ToggleContainerLock { .. } => vec![NativeEffect::ToggleLock],
        BuiltinAction::StackWindow { direction } => vec![NativeEffect::Stack { direction }],
        BuiltinAction::UnstackWindow { .. } => vec![NativeEffect::Unstack],
        BuiltinAction::StackAll => vec![NativeEffect::StackAll],
        BuiltinAction::UnstackAll => vec![NativeEffect::UnstackAll],
        BuiltinAction::CycleStack { direction } => vec![NativeEffect::CycleStack { direction }],
        BuiltinAction::CycleStackIndex { direction } => {
            vec![NativeEffect::CycleStackIndex { direction }]
        }
        BuiltinAction::FocusStackWindow { index } => vec![NativeEffect::FocusStack { index }],
        BuiltinAction::FocusWorkspace { index } => vec![NativeEffect::FocusWorkspace { index }],
        BuiltinAction::CycleFocusWorkspace { direction } => {
            vec![NativeEffect::CycleFocusWorkspace { direction }]
        }
        BuiltinAction::CycleFocusEmptyWorkspace { direction } => {
            vec![NativeEffect::CycleFocusEmptyWorkspace { direction }]
        }
        BuiltinAction::FocusLastWorkspace => vec![NativeEffect::FocusLastWorkspace],
        BuiltinAction::CloseWorkspace => vec![NativeEffect::CloseWorkspace],
        BuiltinAction::FocusMonitor { index } => vec![NativeEffect::FocusMonitor { index }],
        BuiltinAction::CycleFocusMonitor { direction } => {
            vec![NativeEffect::CycleFocusMonitor { direction }]
        }
        BuiltinAction::FocusMonitorAtCursor => vec![NativeEffect::FocusMonitorAtCursor],
        BuiltinAction::FocusWorkspaceOnAllMonitors { index } => {
            vec![NativeEffect::FocusWorkspaceOnAllMonitors { index }]
        }
        BuiltinAction::FocusMonitorWorkspace { monitor, workspace } => {
            vec![NativeEffect::FocusMonitorWorkspace { monitor, workspace }]
        }
        BuiltinAction::CloseWindow { .. } => vec![NativeEffect::CloseWindow],
        BuiltinAction::MinimizeWindow { .. } => vec![NativeEffect::MinimizeWindow],
        BuiltinAction::ForceFocus { .. } => vec![NativeEffect::ForceFocus],
        BuiltinAction::PromoteContainer => vec![NativeEffect::PromoteContainer],
        BuiltinAction::PromoteContainerSwap => vec![NativeEffect::PromoteContainerSwap],
        BuiltinAction::PromoteFocus => vec![NativeEffect::PromoteFocus],
        BuiltinAction::PromoteWindow { direction } => {
            vec![NativeEffect::PromoteWindow { direction }]
        }
        BuiltinAction::NewWorkspace => vec![NativeEffect::CreateWorkspace],
        BuiltinAction::ToggleTiling => vec![NativeEffect::ToggleTiling],
        BuiltinAction::CycleLayout { direction } => vec![NativeEffect::CycleLayout { direction }],
        BuiltinAction::FlipLayout { axis } => vec![NativeEffect::FlipLayout { axis }],
        BuiltinAction::ToggleWorkspaceLayer => vec![NativeEffect::ToggleWorkspaceLayer],
        BuiltinAction::MoveContainerToLastWorkspace => {
            vec![NativeEffect::MoveContainerToLastWorkspace]
        }
        BuiltinAction::SendContainerToLastWorkspace => {
            vec![NativeEffect::SendContainerToLastWorkspace]
        }
        BuiltinAction::MoveContainerToWorkspace { index } => {
            vec![NativeEffect::MoveContainerToWorkspace { index }]
        }
        BuiltinAction::CycleMoveContainerToWorkspace { direction } => {
            vec![NativeEffect::CycleMoveContainerToWorkspace { direction }]
        }
        BuiltinAction::SendContainerToWorkspace { index } => {
            vec![NativeEffect::SendContainerToWorkspace { index }]
        }
        BuiltinAction::CycleSendContainerToWorkspace { direction } => {
            vec![NativeEffect::CycleSendContainerToWorkspace { direction }]
        }
        BuiltinAction::MoveContainerToMonitor { index } => {
            vec![NativeEffect::MoveContainerToMonitor { index }]
        }
        BuiltinAction::CycleMoveContainerToMonitor { direction } => {
            vec![NativeEffect::CycleMoveContainerToMonitor { direction }]
        }
        BuiltinAction::SendContainerToMonitor { index } => {
            vec![NativeEffect::SendContainerToMonitor { index }]
        }
        BuiltinAction::CycleSendContainerToMonitor { direction } => {
            vec![NativeEffect::CycleSendContainerToMonitor { direction }]
        }
        BuiltinAction::MoveContainerToMonitorWorkspace { monitor, workspace } => {
            vec![NativeEffect::MoveContainerToMonitorWorkspace { monitor, workspace }]
        }
        BuiltinAction::SendContainerToMonitorWorkspace { monitor, workspace } => {
            vec![NativeEffect::SendContainerToMonitorWorkspace { monitor, workspace }]
        }
        BuiltinAction::MoveWorkspaceToMonitor { index } => {
            vec![NativeEffect::MoveWorkspaceToMonitor { index }]
        }
        BuiltinAction::CycleMoveWorkspaceToMonitor { direction } => {
            vec![NativeEffect::CycleMoveWorkspaceToMonitor { direction }]
        }
        BuiltinAction::SwapWorkspacesToMonitor { index } => {
            vec![NativeEffect::SwapWorkspacesToMonitor { index }]
        }
        BuiltinAction::PreselectDirection { direction } => {
            vec![NativeEffect::PreselectDirection { direction }]
        }
        BuiltinAction::CancelPreselect => vec![NativeEffect::CancelPreselect],
        BuiltinAction::Retile => vec![NativeEffect::Retile],
        BuiltinAction::RetileWithResizeDimensions => {
            vec![NativeEffect::RetileWithResizeDimensions]
        }
        BuiltinAction::ManageFocusedWindow => vec![NativeEffect::ManageFocusedWindow],
        BuiltinAction::UnmanageFocusedWindow => vec![NativeEffect::UnmanageFocusedWindow],
        BuiltinAction::AdjustContainerPadding { sizing, adjustment } => {
            vec![NativeEffect::AdjustContainerPadding { sizing, adjustment }]
        }
        BuiltinAction::AdjustWorkspacePadding { sizing, adjustment } => {
            vec![NativeEffect::AdjustWorkspacePadding { sizing, adjustment }]
        }
        BuiltinAction::ToggleMouseFollowsFocus => vec![NativeEffect::ToggleMouseFollowsFocus],
        BuiltinAction::SetMouseFollowsFocus { enabled } => {
            vec![NativeEffect::SetMouseFollowsFocus { enabled }]
        }
        BuiltinAction::ToggleWindowContainerBehaviour => {
            vec![NativeEffect::ToggleWindowContainerBehaviour]
        }
        BuiltinAction::ToggleFloatOverride => vec![NativeEffect::ToggleFloatOverride],
        BuiltinAction::ToggleWorkspaceWindowContainerBehaviour => {
            vec![NativeEffect::ToggleWorkspaceWindowContainerBehaviour]
        }
        BuiltinAction::ToggleWorkspaceFloatOverride => {
            vec![NativeEffect::ToggleWorkspaceFloatOverride]
        }
        BuiltinAction::ToggleCrossMonitorMoveBehaviour => {
            vec![NativeEffect::ToggleCrossMonitorMoveBehaviour]
        }
        BuiltinAction::ToggleMonocleFocusBehaviour => {
            vec![NativeEffect::ToggleMonocleFocusBehaviour]
        }
        BuiltinAction::TogglePause => vec![NativeEffect::TogglePause],
        BuiltinAction::SetFocusedContainerPadding { size } => {
            vec![NativeEffect::SetFocusedContainerPadding { size }]
        }
        BuiltinAction::SetFocusedWorkspacePadding { size } => {
            vec![NativeEffect::SetFocusedWorkspacePadding { size }]
        }
        BuiltinAction::SetContainerPadding {
            monitor,
            workspace,
            size,
        } => vec![NativeEffect::SetContainerPadding {
            monitor,
            workspace,
            size,
        }],
        BuiltinAction::SetWorkspacePadding {
            monitor,
            workspace,
            size,
        } => vec![NativeEffect::SetWorkspacePadding {
            monitor,
            workspace,
            size,
        }],
        BuiltinAction::SetWorkspaceTiling {
            monitor,
            workspace,
            tile,
        } => vec![NativeEffect::SetWorkspaceTiling {
            monitor,
            workspace,
            tile,
        }],
        BuiltinAction::SetMonitorWorkspaceLayout {
            monitor,
            workspace,
            layout,
        } => vec![NativeEffect::SetMonitorWorkspaceLayout {
            monitor,
            workspace,
            layout,
        }],
        BuiltinAction::EnsureWorkspaces { monitor, count } => {
            vec![NativeEffect::EnsureWorkspaces { monitor, count }]
        }
        BuiltinAction::ClearWorkspaceLayoutRules { monitor, workspace } => {
            vec![NativeEffect::ClearWorkspaceLayoutRules { monitor, workspace }]
        }
        BuiltinAction::SetScrollingColumns { columns } => {
            vec![NativeEffect::SetScrollingColumns { columns }]
        }
        BuiltinAction::LockContainer {
            monitor,
            workspace,
            container,
        } => vec![NativeEffect::LockContainer {
            monitor,
            workspace,
            container,
        }],
        BuiltinAction::UnlockContainer {
            monitor,
            workspace,
            container,
        } => vec![NativeEffect::UnlockContainer {
            monitor,
            workspace,
            container,
        }],
        BuiltinAction::ToggleTitleBars => vec![NativeEffect::ToggleTitleBars],
        BuiltinAction::EnforceWorkspaceRules => vec![NativeEffect::EnforceWorkspaceRules],
        BuiltinAction::AddSessionFloatRule => vec![NativeEffect::AddSessionFloatRule],
        BuiltinAction::ClearSessionFloatRules => vec![NativeEffect::ClearSessionFloatRules],
        BuiltinAction::ResizeWindowEdge { direction, delta } => {
            vec![NativeEffect::ResizeEdge { direction, delta }]
        }
        BuiltinAction::SetWindowHidingBehaviour { behaviour } => {
            vec![NativeEffect::SetWindowHidingBehaviour { behaviour }]
        }
        BuiltinAction::SetCrossMonitorMoveBehaviour { behaviour } => {
            vec![NativeEffect::SetCrossMonitorMoveBehaviour { behaviour }]
        }
        BuiltinAction::SetMonocleFocusBehaviour { behaviour } => {
            vec![NativeEffect::SetMonocleFocusBehaviour { behaviour }]
        }
        BuiltinAction::SetUnmanagedWindowOperationBehaviour { behaviour } => {
            vec![NativeEffect::SetUnmanagedWindowOperationBehaviour { behaviour }]
        }
        BuiltinAction::SetFocusFollowsMouse {
            implementation,
            enabled,
        } => vec![NativeEffect::SetFocusFollowsMouse {
            implementation,
            enabled,
        }],
        BuiltinAction::ToggleFocusFollowsMouse { implementation } => {
            vec![NativeEffect::ToggleFocusFollowsMouse { implementation }]
        }
        BuiltinAction::AddWorkspaceLayoutRule {
            monitor,
            workspace,
            at_container_count,
            layout,
        } => vec![NativeEffect::AddWorkspaceLayoutRule {
            monitor,
            workspace,
            at_container_count,
            layout,
        }],
        BuiltinAction::ResizeWindowByStep { .. }
        | BuiltinAction::ResizeWindowEdgeByStep { .. }
        | BuiltinAction::FocusNamedWorkspace { .. }
        | BuiltinAction::MoveContainerToNamedWorkspace { .. }
        | BuiltinAction::SendContainerToNamedWorkspace { .. }
        | BuiltinAction::SetNamedWorkspaceContainerPadding { .. }
        | BuiltinAction::SetNamedWorkspacePadding { .. }
        | BuiltinAction::SetNamedWorkspaceTiling { .. }
        | BuiltinAction::SetNamedWorkspaceLayout { .. }
        | BuiltinAction::SetNamedWorkspaceCustomLayout { .. }
        | BuiltinAction::AddNamedWorkspaceLayoutRule { .. }
        | BuiltinAction::AddNamedWorkspaceCustomLayoutRule { .. }
        | BuiltinAction::ClearNamedWorkspaceLayoutRules { .. } => {
            unreachable!("contextual actions resolve before effects")
        }
        BuiltinAction::EnsureNamedWorkspaces { monitor, names } => {
            vec![NativeEffect::EnsureNamedWorkspaces { monitor, names }]
        }
        BuiltinAction::SetWorkspaceName {
            monitor,
            workspace,
            name,
        } => vec![NativeEffect::SetWorkspaceName {
            monitor,
            workspace,
            name,
        }],
        BuiltinAction::SetLayoutRatios { columns, rows } => {
            vec![NativeEffect::SetLayoutRatios { columns, rows }]
        }
        BuiltinAction::SetCustomLayout { path } => vec![NativeEffect::SetCustomLayout { path }],
        BuiltinAction::SetWorkspaceCustomLayout {
            monitor,
            workspace,
            path,
        } => vec![NativeEffect::SetWorkspaceCustomLayout {
            monitor,
            workspace,
            path,
        }],
        BuiltinAction::AddWorkspaceCustomLayoutRule {
            monitor,
            workspace,
            at_container_count,
            path,
        } => vec![NativeEffect::AddWorkspaceCustomLayoutRule {
            monitor,
            workspace,
            at_container_count,
            path,
        }],
        BuiltinAction::EagerFocus { exe } => vec![NativeEffect::EagerFocus { exe }],
        BuiltinAction::RemoveTitleBar { identifier, id } => {
            vec![NativeEffect::RemoveTitleBar { identifier, id }]
        }
    }
}
