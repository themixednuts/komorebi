use std::sync::Arc;

use super::ActionResult;
use super::ActionSnapshot;
use super::BuiltinAction;
use super::NativeEffect;
use super::Unavailability;
use crate::action::WorkspaceLocation;

pub(super) fn resolve(
    snapshot: &ActionSnapshot,
    action: &BuiltinAction,
) -> Result<BuiltinAction, Unavailability> {
    match action {
        BuiltinAction::SetGlobalWorkAreaOffset { .. } => Ok(action.clone()),
        BuiltinAction::SetMonitorWorkAreaOffset { monitor, .. } => snapshot
            .configuration
            .work_area
            .monitor(*monitor)
            .map(|_| action.clone())
            .ok_or(Unavailability::UnknownMonitor),
        BuiltinAction::SetWorkspaceWorkAreaOffset {
            monitor, workspace, ..
        } => {
            if snapshot
                .configuration
                .work_area
                .workspace(*monitor, *workspace)
                .is_some()
            {
                Ok(action.clone())
            } else if snapshot.configuration.work_area.monitor(*monitor).is_none() {
                Err(Unavailability::UnknownMonitor)
            } else {
                Err(Unavailability::UnknownWorkspace)
            }
        }
        BuiltinAction::ToggleWindowBasedWorkAreaOffset => focused(snapshot)
            .map(|_| action.clone())
            .ok_or(Unavailability::NoFocusedWorkspace),
        _ => unreachable!("the transition dispatcher only passes work-area actions"),
    }
}

pub(super) fn apply(snapshot: &mut ActionSnapshot, action: &BuiltinAction) {
    match action {
        BuiltinAction::SetGlobalWorkAreaOffset { offset } => {
            Arc::make_mut(&mut snapshot.configuration.work_area).global = Some(*offset);
        }
        BuiltinAction::SetMonitorWorkAreaOffset { monitor, offset } => {
            let Some(configuration) =
                Arc::make_mut(&mut snapshot.configuration.work_area).monitor_mut(*monitor)
            else {
                unreachable!("work-area targets are validated before logical transition");
            };
            configuration.offset = Some(*offset);
        }
        BuiltinAction::SetWorkspaceWorkAreaOffset {
            monitor,
            workspace,
            offset,
        } => {
            let Some(configuration) = Arc::make_mut(&mut snapshot.configuration.work_area)
                .workspace_mut(*monitor, *workspace)
            else {
                unreachable!("work-area targets are validated before logical transition");
            };
            configuration.offset = Some(*offset);
        }
        BuiltinAction::ToggleWindowBasedWorkAreaOffset => {
            let Some(location) = snapshot.focused_workspace else {
                unreachable!("focused-workspace actions are validated before logical transition");
            };
            let Some(configuration) = Arc::make_mut(&mut snapshot.configuration.work_area)
                .workspace_mut(location.monitor(), location.workspace())
            else {
                unreachable!("focused work-area state is observed before logical transition");
            };
            configuration.window_based = !configuration.window_based;
        }
        _ => unreachable!("the transition dispatcher only passes work-area actions"),
    }
}

pub(super) fn logical_result(snapshot: &ActionSnapshot, action: &BuiltinAction) -> ActionResult {
    match action {
        BuiltinAction::SetGlobalWorkAreaOffset { .. } => ActionResult::GlobalWorkAreaOffsetSet,
        BuiltinAction::SetMonitorWorkAreaOffset { .. } => ActionResult::MonitorWorkAreaOffsetSet,
        BuiltinAction::SetWorkspaceWorkAreaOffset { .. } => {
            ActionResult::WorkspaceWorkAreaOffsetSet
        }
        BuiltinAction::ToggleWindowBasedWorkAreaOffset => {
            let Some((_, enabled)) = focused(snapshot) else {
                unreachable!("work-area target is valid after transition");
            };
            ActionResult::WindowBasedWorkAreaOffsetToggled { enabled }
        }
        _ => unreachable!("the transition dispatcher only passes work-area actions"),
    }
}

pub(super) fn effects(snapshot: &ActionSnapshot, action: &BuiltinAction) -> Vec<NativeEffect> {
    match action {
        BuiltinAction::SetGlobalWorkAreaOffset { offset } => {
            vec![NativeEffect::SetGlobalWorkAreaOffset { offset: *offset }]
        }
        BuiltinAction::SetMonitorWorkAreaOffset { monitor, offset } => {
            vec![NativeEffect::SetMonitorWorkAreaOffset {
                monitor: *monitor,
                offset: *offset,
            }]
        }
        BuiltinAction::SetWorkspaceWorkAreaOffset {
            monitor,
            workspace,
            offset,
        } => vec![NativeEffect::SetWorkspaceWorkAreaOffset {
            location: WorkspaceLocation::new(*monitor, *workspace),
            offset: *offset,
        }],
        BuiltinAction::ToggleWindowBasedWorkAreaOffset => {
            let Some((location, enabled)) = focused(snapshot) else {
                unreachable!("work-area target is valid after transition");
            };
            vec![NativeEffect::SetWindowBasedWorkAreaOffset { location, enabled }]
        }
        _ => unreachable!("the transition dispatcher only passes work-area actions"),
    }
}

fn focused(snapshot: &ActionSnapshot) -> Option<(WorkspaceLocation, bool)> {
    let location = snapshot.focused_workspace?;
    let configuration = snapshot
        .configuration
        .work_area
        .workspace(location.monitor(), location.workspace())?;
    Some((location, configuration.window_based))
}
