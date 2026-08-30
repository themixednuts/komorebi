use komorebi_protocol as protocol;
use thiserror::Error;

mod arguments;

pub use arguments::ArgumentBindingError;
pub use arguments::ScalarKind;
use arguments::ValidatedArguments;

use crate::action::BuiltinAction;
use crate::action::BuiltinActionKind;
use crate::action::ConfirmationToken;
use crate::action::ContainerIndex;
use crate::action::InvokeAction;
use crate::action::MonitorIndex;
use crate::action::StackIndex;
use crate::action::WorkspaceIndex;
use crate::action::id::ParameterId;

/// Converts an invocation from one exact authority-scoped catalog into the
/// closed manager action domain.
///
/// No protocol argument collection survives this boundary.
///
/// # Errors
///
/// Returns a closed submission rejection for stale catalog identity,
/// unavailable offers, or arguments that do not match the advertised action
/// schema exactly.
pub fn bind(
    catalog: &protocol::CatalogSnapshot,
    invocation: &protocol::ActionInvocation,
) -> Result<InvokeAction, InvocationBindingError> {
    validate_identity(catalog, invocation)?;
    let (definition, offer) = catalog
        .definitions()
        .iter()
        .zip(catalog.offers())
        .find(|(definition, _)| definition.key() == invocation.offer().action())
        .ok_or(InvocationBindingError::Rejected(
            protocol::InvocationRejection::StaleOffer,
        ))?;
    let fingerprint = protocol::CatalogCodec::definition_fingerprint(definition)?;
    if offer.reference().contract() != fingerprint || invocation.offer().contract() != fingerprint {
        return Err(InvocationBindingError::Rejected(
            protocol::InvocationRejection::StaleOffer,
        ));
    }
    match offer.availability() {
        protocol::ActionAvailability::Available => {}
        protocol::ActionAvailability::Unavailable(protocol::ActionUnavailability::Unauthorized) => {
            return Err(InvocationBindingError::Rejected(
                protocol::InvocationRejection::Unauthorized,
            ));
        }
        protocol::ActionAvailability::Unavailable(reason) => {
            return Err(InvocationBindingError::Rejected(
                protocol::InvocationRejection::Unavailable(reason),
            ));
        }
    }
    let kind = BuiltinActionKind::ALL
        .into_iter()
        .find(|kind| kind.id().as_str() == definition.key().id().as_str())
        .filter(|kind| {
            kind.definition().schema_version.get() == definition.key().schema_version().get()
        })
        .ok_or(InvocationBindingError::Rejected(
            protocol::InvocationRejection::StaleOffer,
        ))?;
    let arguments = ValidatedArguments::new(kind.definition(), invocation.arguments())?;
    validate_dynamic_choices(offer, invocation.arguments())?;
    let action = bind_builtin(kind, &arguments)?;
    let confirmation = invocation
        .confirmation()
        .map(protocol::ConfirmationChallengeId::into_bytes)
        .map(ConfirmationToken::from_bytes);

    Ok(InvokeAction {
        invocation_id: invocation.invocation_id(),
        expected_state: invocation.expected_state(),
        action,
        confirmation,
    })
}

fn validate_dynamic_choices(
    offer: &protocol::ActionOffer,
    arguments: &protocol::ActionArguments,
) -> Result<(), InvocationBindingError> {
    for group in offer.dynamic_choices() {
        let Some(argument) = arguments.values().get(group.parameter()) else {
            continue;
        };
        let protocol::ActionArgument::Scalar(value) = argument else {
            return Err(InvocationBindingError::Rejected(
                protocol::InvocationRejection::InvalidArguments,
            ));
        };
        if !group.choices().contains(value) {
            return Err(InvocationBindingError::Rejected(
                protocol::InvocationRejection::StaleOffer,
            ));
        }
    }
    Ok(())
}

fn validate_identity(
    catalog: &protocol::CatalogSnapshot,
    invocation: &protocol::ActionInvocation,
) -> Result<(), InvocationBindingError> {
    if invocation.expected_state().epoch() != catalog.state().epoch()
        || invocation.offer().catalog().epoch() != catalog.stamp().epoch()
    {
        return Err(InvocationBindingError::Rejected(
            protocol::InvocationRejection::StaleEpoch,
        ));
    }
    if invocation.expected_state() != catalog.state() {
        return Err(InvocationBindingError::Rejected(
            protocol::InvocationRejection::StaleState {
                current: catalog.state(),
            },
        ));
    }
    if invocation.offer().catalog() != catalog.stamp() {
        return Err(InvocationBindingError::Rejected(
            protocol::InvocationRejection::StaleCatalog {
                current: catalog.stamp(),
            },
        ));
    }
    Ok(())
}

fn bind_builtin(
    kind: BuiltinActionKind,
    args: &ValidatedArguments<'_>,
) -> Result<BuiltinAction, ArgumentBindingError> {
    use BuiltinAction as A;
    use BuiltinActionKind as K;

    Ok(match kind {
        K::FocusWindow => A::FocusWindow {
            direction: args.direction(ParameterId::DIRECTION)?,
        },
        K::MoveWindow => A::MoveWindow {
            direction: args.direction(ParameterId::DIRECTION)?,
        },
        K::ResizeWindow => A::ResizeWindow {
            axis: args.axis(ParameterId::AXIS)?,
            delta: args.pixels(ParameterId::DELTA)?,
        },
        K::ResizeWindowByStep => A::ResizeWindowByStep {
            axis: args.axis(ParameterId::AXIS)?,
            sizing: args.sizing(ParameterId::SIZING)?,
        },
        K::SetWorkspaceLayout => A::SetWorkspaceLayout {
            workspace: args.workspace_selector(ParameterId::WORKSPACE)?,
            layout: args.layout(ParameterId::LAYOUT)?,
        },
        K::ToggleWindowFloat => A::ToggleWindowFloat {
            window: args.window_selector(ParameterId::WINDOW)?,
        },
        K::CycleFocusWindow => A::CycleFocusWindow {
            direction: args.cycle(ParameterId::CYCLE)?,
        },
        K::CycleMoveWindow => A::CycleMoveWindow {
            direction: args.cycle(ParameterId::CYCLE)?,
        },
        K::ToggleWindowMonocle => A::ToggleWindowMonocle {
            window: args.window_selector(ParameterId::WINDOW)?,
        },
        K::ToggleWindowMaximize => A::ToggleWindowMaximize {
            window: args.window_selector(ParameterId::WINDOW)?,
        },
        K::ToggleContainerLock => A::ToggleContainerLock {
            window: args.window_selector(ParameterId::WINDOW)?,
        },
        K::StackWindow => A::StackWindow {
            direction: args.direction(ParameterId::DIRECTION)?,
        },
        K::UnstackWindow => A::UnstackWindow {
            window: args.window_selector(ParameterId::WINDOW)?,
        },
        K::StackAll => A::StackAll,
        K::UnstackAll => A::UnstackAll,
        K::CycleStack => A::CycleStack {
            direction: args.cycle(ParameterId::CYCLE)?,
        },
        K::CycleStackIndex => A::CycleStackIndex {
            direction: args.cycle(ParameterId::CYCLE)?,
        },
        K::FocusStackWindow => A::FocusStackWindow {
            index: StackIndex::new(args.usize(ParameterId::INDEX)?),
        },
        K::FocusWorkspace => A::FocusWorkspace {
            index: WorkspaceIndex::new(args.usize(ParameterId::INDEX)?),
        },
        K::CycleFocusWorkspace => A::CycleFocusWorkspace {
            direction: args.cycle(ParameterId::CYCLE)?,
        },
        K::CycleFocusEmptyWorkspace => A::CycleFocusEmptyWorkspace {
            direction: args.cycle(ParameterId::CYCLE)?,
        },
        K::FocusLastWorkspace => A::FocusLastWorkspace,
        K::CloseWorkspace => A::CloseWorkspace,
        K::FocusMonitor => A::FocusMonitor {
            index: MonitorIndex::new(args.usize(ParameterId::INDEX)?),
        },
        K::CycleFocusMonitor => A::CycleFocusMonitor {
            direction: args.cycle(ParameterId::CYCLE)?,
        },
        K::FocusMonitorAtCursor => A::FocusMonitorAtCursor,
        K::FocusWorkspaceOnAllMonitors => A::FocusWorkspaceOnAllMonitors {
            index: WorkspaceIndex::new(args.usize(ParameterId::INDEX)?),
        },
        K::FocusMonitorWorkspace => A::FocusMonitorWorkspace {
            monitor: MonitorIndex::new(args.usize(ParameterId::MONITOR)?),
            workspace: WorkspaceIndex::new(args.usize(ParameterId::INDEX)?),
        },
        K::CloseWindow => A::CloseWindow {
            window: args.window_selector(ParameterId::WINDOW)?,
        },
        K::MinimizeWindow => A::MinimizeWindow {
            window: args.window_selector(ParameterId::WINDOW)?,
        },
        K::ForceFocus => A::ForceFocus {
            window: args.window_selector(ParameterId::WINDOW)?,
        },
        K::PromoteContainer => A::PromoteContainer,
        K::PromoteContainerSwap => A::PromoteContainerSwap,
        K::PromoteFocus => A::PromoteFocus,
        K::PromoteWindow => A::PromoteWindow {
            direction: args.direction(ParameterId::DIRECTION)?,
        },
        K::NewWorkspace => A::NewWorkspace,
        K::ToggleTiling => A::ToggleTiling,
        K::CycleLayout => A::CycleLayout {
            direction: args.cycle(ParameterId::CYCLE)?,
        },
        K::FlipLayout => A::FlipLayout {
            axis: args.axis(ParameterId::AXIS)?,
        },
        K::ToggleWorkspaceLayer => A::ToggleWorkspaceLayer,
        K::MoveContainerToLastWorkspace => A::MoveContainerToLastWorkspace,
        K::SendContainerToLastWorkspace => A::SendContainerToLastWorkspace,
        K::MoveContainerToWorkspace => A::MoveContainerToWorkspace {
            index: WorkspaceIndex::new(args.usize(ParameterId::INDEX)?),
        },
        K::CycleMoveContainerToWorkspace => A::CycleMoveContainerToWorkspace {
            direction: args.cycle(ParameterId::CYCLE)?,
        },
        K::SendContainerToWorkspace => A::SendContainerToWorkspace {
            index: WorkspaceIndex::new(args.usize(ParameterId::INDEX)?),
        },
        K::CycleSendContainerToWorkspace => A::CycleSendContainerToWorkspace {
            direction: args.cycle(ParameterId::CYCLE)?,
        },
        K::MoveContainerToMonitor => A::MoveContainerToMonitor {
            index: MonitorIndex::new(args.usize(ParameterId::INDEX)?),
        },
        K::CycleMoveContainerToMonitor => A::CycleMoveContainerToMonitor {
            direction: args.cycle(ParameterId::CYCLE)?,
        },
        K::SendContainerToMonitor => A::SendContainerToMonitor {
            index: MonitorIndex::new(args.usize(ParameterId::INDEX)?),
        },
        K::CycleSendContainerToMonitor => A::CycleSendContainerToMonitor {
            direction: args.cycle(ParameterId::CYCLE)?,
        },
        K::MoveContainerToMonitorWorkspace => A::MoveContainerToMonitorWorkspace {
            monitor: MonitorIndex::new(args.usize(ParameterId::MONITOR)?),
            workspace: WorkspaceIndex::new(args.usize(ParameterId::INDEX)?),
        },
        K::SendContainerToMonitorWorkspace => A::SendContainerToMonitorWorkspace {
            monitor: MonitorIndex::new(args.usize(ParameterId::MONITOR)?),
            workspace: WorkspaceIndex::new(args.usize(ParameterId::INDEX)?),
        },
        K::MoveWorkspaceToMonitor => A::MoveWorkspaceToMonitor {
            index: MonitorIndex::new(args.usize(ParameterId::INDEX)?),
        },
        K::CycleMoveWorkspaceToMonitor => A::CycleMoveWorkspaceToMonitor {
            direction: args.cycle(ParameterId::CYCLE)?,
        },
        K::SwapWorkspacesToMonitor => A::SwapWorkspacesToMonitor {
            index: MonitorIndex::new(args.usize(ParameterId::INDEX)?),
        },
        K::PreselectDirection => A::PreselectDirection {
            direction: args.direction(ParameterId::DIRECTION)?,
        },
        K::CancelPreselect => A::CancelPreselect,
        K::Retile => A::Retile,
        K::RetileWithResizeDimensions => A::RetileWithResizeDimensions,
        K::ManageFocusedWindow => A::ManageFocusedWindow,
        K::UnmanageFocusedWindow => A::UnmanageFocusedWindow,
        K::AdjustContainerPadding => A::AdjustContainerPadding {
            sizing: args.sizing(ParameterId::SIZING)?,
            adjustment: args.i32(ParameterId::ADJUSTMENT)?,
        },
        K::AdjustWorkspacePadding => A::AdjustWorkspacePadding {
            sizing: args.sizing(ParameterId::SIZING)?,
            adjustment: args.i32(ParameterId::ADJUSTMENT)?,
        },
        K::ToggleMouseFollowsFocus => A::ToggleMouseFollowsFocus,
        K::SetMouseFollowsFocus => A::SetMouseFollowsFocus {
            enabled: args.boolean(ParameterId::ENABLED)?,
        },
        K::ToggleWindowContainerBehaviour => A::ToggleWindowContainerBehaviour,
        K::ToggleFloatOverride => A::ToggleFloatOverride,
        K::ToggleWorkspaceWindowContainerBehaviour => A::ToggleWorkspaceWindowContainerBehaviour,
        K::ToggleWorkspaceFloatOverride => A::ToggleWorkspaceFloatOverride,
        K::ToggleCrossMonitorMoveBehaviour => A::ToggleCrossMonitorMoveBehaviour,
        K::ToggleMonocleFocusBehaviour => A::ToggleMonocleFocusBehaviour,
        K::TogglePause => A::TogglePause,
        K::SetFocusedContainerPadding => A::SetFocusedContainerPadding {
            size: args.i32(ParameterId::SIZE)?,
        },
        K::SetFocusedWorkspacePadding => A::SetFocusedWorkspacePadding {
            size: args.i32(ParameterId::SIZE)?,
        },
        K::SetContainerPadding => A::SetContainerPadding {
            monitor: MonitorIndex::new(args.usize(ParameterId::MONITOR)?),
            workspace: WorkspaceIndex::new(args.usize(ParameterId::INDEX)?),
            size: args.i32(ParameterId::SIZE)?,
        },
        K::SetWorkspacePadding => A::SetWorkspacePadding {
            monitor: MonitorIndex::new(args.usize(ParameterId::MONITOR)?),
            workspace: WorkspaceIndex::new(args.usize(ParameterId::INDEX)?),
            size: args.i32(ParameterId::SIZE)?,
        },
        K::SetWorkspaceTiling => A::SetWorkspaceTiling {
            monitor: MonitorIndex::new(args.usize(ParameterId::MONITOR)?),
            workspace: WorkspaceIndex::new(args.usize(ParameterId::INDEX)?),
            tile: args.boolean(ParameterId::ENABLED)?,
        },
        K::SetMonitorWorkspaceLayout => A::SetMonitorWorkspaceLayout {
            monitor: MonitorIndex::new(args.usize(ParameterId::MONITOR)?),
            workspace: WorkspaceIndex::new(args.usize(ParameterId::INDEX)?),
            layout: args.layout(ParameterId::LAYOUT)?,
        },
        K::EnsureWorkspaces => A::EnsureWorkspaces {
            monitor: MonitorIndex::new(args.usize(ParameterId::MONITOR)?),
            count: args.usize(ParameterId::COUNT)?,
        },
        K::ClearWorkspaceLayoutRules => A::ClearWorkspaceLayoutRules {
            monitor: MonitorIndex::new(args.usize(ParameterId::MONITOR)?),
            workspace: WorkspaceIndex::new(args.usize(ParameterId::INDEX)?),
        },
        K::SetScrollingColumns => A::SetScrollingColumns {
            columns: args.nonzero_usize(ParameterId::COLUMNS)?,
        },
        K::LockContainer => A::LockContainer {
            monitor: MonitorIndex::new(args.usize(ParameterId::MONITOR)?),
            workspace: WorkspaceIndex::new(args.usize(ParameterId::INDEX)?),
            container: ContainerIndex::new(args.usize(ParameterId::CONTAINER)?),
        },
        K::UnlockContainer => A::UnlockContainer {
            monitor: MonitorIndex::new(args.usize(ParameterId::MONITOR)?),
            workspace: WorkspaceIndex::new(args.usize(ParameterId::INDEX)?),
            container: ContainerIndex::new(args.usize(ParameterId::CONTAINER)?),
        },
        K::ToggleTitleBars => A::ToggleTitleBars,
        K::EnforceWorkspaceRules => A::EnforceWorkspaceRules,
        K::AddSessionFloatRule => A::AddSessionFloatRule,
        K::ClearSessionFloatRules => A::ClearSessionFloatRules,
        K::ResizeWindowEdge => A::ResizeWindowEdge {
            direction: args.direction(ParameterId::DIRECTION)?,
            delta: args.pixels(ParameterId::DELTA)?,
        },
        K::ResizeWindowEdgeByStep => A::ResizeWindowEdgeByStep {
            direction: args.direction(ParameterId::DIRECTION)?,
            sizing: args.sizing(ParameterId::SIZING)?,
        },
        K::SetWindowHidingBehaviour => A::SetWindowHidingBehaviour {
            behaviour: args.hiding_behaviour(ParameterId::BEHAVIOUR)?,
        },
        K::SetCrossMonitorMoveBehaviour => A::SetCrossMonitorMoveBehaviour {
            behaviour: args.move_behaviour(ParameterId::BEHAVIOUR)?,
        },
        K::SetMonocleFocusBehaviour => A::SetMonocleFocusBehaviour {
            behaviour: args.monocle_behaviour(ParameterId::BEHAVIOUR)?,
        },
        K::SetUnmanagedWindowOperationBehaviour => A::SetUnmanagedWindowOperationBehaviour {
            behaviour: args.operation_behaviour(ParameterId::BEHAVIOUR)?,
        },
        K::SetFocusFollowsMouse => A::SetFocusFollowsMouse {
            implementation: args.ffm_implementation(ParameterId::IMPLEMENTATION)?,
            enabled: args.boolean(ParameterId::ENABLED)?,
        },
        K::ToggleFocusFollowsMouse => A::ToggleFocusFollowsMouse {
            implementation: args.ffm_implementation(ParameterId::IMPLEMENTATION)?,
        },
        K::AddWorkspaceLayoutRule => A::AddWorkspaceLayoutRule {
            monitor: MonitorIndex::new(args.usize(ParameterId::MONITOR)?),
            workspace: WorkspaceIndex::new(args.usize(ParameterId::INDEX)?),
            at_container_count: args.usize(ParameterId::AT_COUNT)?,
            layout: args.layout(ParameterId::LAYOUT)?,
        },
        K::FocusNamedWorkspace => A::FocusNamedWorkspace {
            name: args.workspace_name(ParameterId::NAME)?,
        },
        K::MoveContainerToNamedWorkspace => A::MoveContainerToNamedWorkspace {
            name: args.workspace_name(ParameterId::NAME)?,
        },
        K::SendContainerToNamedWorkspace => A::SendContainerToNamedWorkspace {
            name: args.workspace_name(ParameterId::NAME)?,
        },
        K::SetNamedWorkspaceContainerPadding => A::SetNamedWorkspaceContainerPadding {
            name: args.workspace_name(ParameterId::NAME)?,
            size: args.i32(ParameterId::SIZE)?,
        },
        K::SetNamedWorkspacePadding => A::SetNamedWorkspacePadding {
            name: args.workspace_name(ParameterId::NAME)?,
            size: args.i32(ParameterId::SIZE)?,
        },
        K::SetNamedWorkspaceTiling => A::SetNamedWorkspaceTiling {
            name: args.workspace_name(ParameterId::NAME)?,
            tile: args.boolean(ParameterId::ENABLED)?,
        },
        K::SetNamedWorkspaceLayout => A::SetNamedWorkspaceLayout {
            name: args.workspace_name(ParameterId::NAME)?,
            layout: args.layout(ParameterId::LAYOUT)?,
        },
        K::SetNamedWorkspaceCustomLayout => A::SetNamedWorkspaceCustomLayout {
            name: args.workspace_name(ParameterId::NAME)?,
            path: args.windows_path(ParameterId::PATH)?,
        },
        K::AddNamedWorkspaceLayoutRule => A::AddNamedWorkspaceLayoutRule {
            name: args.workspace_name(ParameterId::NAME)?,
            at_container_count: args.usize(ParameterId::AT_COUNT)?,
            layout: args.layout(ParameterId::LAYOUT)?,
        },
        K::AddNamedWorkspaceCustomLayoutRule => A::AddNamedWorkspaceCustomLayoutRule {
            name: args.workspace_name(ParameterId::NAME)?,
            at_container_count: args.usize(ParameterId::AT_COUNT)?,
            path: args.windows_path(ParameterId::PATH)?,
        },
        K::ClearNamedWorkspaceLayoutRules => A::ClearNamedWorkspaceLayoutRules {
            name: args.workspace_name(ParameterId::NAME)?,
        },
        K::EnsureNamedWorkspaces => A::EnsureNamedWorkspaces {
            monitor: MonitorIndex::new(args.usize(ParameterId::MONITOR)?),
            names: args.workspace_names(ParameterId::NAMES)?,
        },
        K::SetWorkspaceName => A::SetWorkspaceName {
            monitor: MonitorIndex::new(args.usize(ParameterId::MONITOR)?),
            workspace: WorkspaceIndex::new(args.usize(ParameterId::INDEX)?),
            name: args.workspace_name(ParameterId::NAME)?,
        },
        K::SetLayoutRatios => A::SetLayoutRatios {
            columns: args.ratios(ParameterId::COLUMN_RATIOS)?,
            rows: args.ratios(ParameterId::ROW_RATIOS)?,
        },
        K::SetCustomLayout => A::SetCustomLayout {
            path: args.windows_path(ParameterId::PATH)?,
        },
        K::SetWorkspaceCustomLayout => A::SetWorkspaceCustomLayout {
            monitor: MonitorIndex::new(args.usize(ParameterId::MONITOR)?),
            workspace: WorkspaceIndex::new(args.usize(ParameterId::INDEX)?),
            path: args.windows_path(ParameterId::PATH)?,
        },
        K::AddWorkspaceCustomLayoutRule => A::AddWorkspaceCustomLayoutRule {
            monitor: MonitorIndex::new(args.usize(ParameterId::MONITOR)?),
            workspace: WorkspaceIndex::new(args.usize(ParameterId::INDEX)?),
            at_container_count: args.usize(ParameterId::AT_COUNT)?,
            path: args.windows_path(ParameterId::PATH)?,
        },
        K::EagerFocus => A::EagerFocus {
            exe: args.text(ParameterId::EXE)?.to_owned(),
        },
        K::RemoveTitleBar => A::RemoveTitleBar {
            identifier: args.application_identifier(ParameterId::IDENTIFIER)?,
            id: args.text(ParameterId::EXE)?.to_owned(),
        },
    })
}

#[derive(Debug, Error)]
pub enum InvocationBindingError {
    #[error("invocation rejected: {0:?}")]
    Rejected(protocol::InvocationRejection),
    #[error("invalid invocation arguments: {0}")]
    Arguments(#[from] ArgumentBindingError),
    #[error("could not verify the catalog definition contract: {0}")]
    CatalogContract(#[from] protocol::CommandCodecError),
}
