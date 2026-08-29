use super::*;

pub const SET_WORKSPACE_LAYOUT: ActionDefinition = ActionDefinition {
    id: ActionId::SET_WORKSPACE_LAYOUT,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::SetWorkspaceLayout,
    category: ActionCategory::Workspace,
    title: "Set workspace layout",
    description: "Set the focused workspace to a built-in layout",
    keywords: &["layout", "workspace", "bsp", "columns"],
    parameters: LAYOUT,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const FOCUS_WORKSPACE: ActionDefinition = ActionDefinition {
    id: ActionId::FOCUS_WORKSPACE,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::FocusWorkspace,
    category: ActionCategory::Workspace,
    title: "Focus workspace",
    description: "Focus a workspace by index on the target monitor",
    keywords: &["workspace", "focus", "index"],
    parameters: INDEX,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const CYCLE_FOCUS_WORKSPACE: ActionDefinition = ActionDefinition {
    id: ActionId::CYCLE_FOCUS_WORKSPACE,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::CycleFocusWorkspace,
    category: ActionCategory::Workspace,
    title: "Cycle focus workspace",
    description: "Focus the next or previous workspace on the target monitor",
    keywords: &["workspace", "focus", "cycle"],
    parameters: CYCLE,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const CYCLE_FOCUS_EMPTY_WORKSPACE: ActionDefinition = ActionDefinition {
    id: ActionId::CYCLE_FOCUS_EMPTY_WORKSPACE,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::CycleFocusEmptyWorkspace,
    category: ActionCategory::Workspace,
    title: "Cycle focus empty workspace",
    description: "Focus the next or previous empty workspace on the target monitor",
    keywords: &["workspace", "empty", "cycle"],
    parameters: CYCLE,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const FOCUS_LAST_WORKSPACE: ActionDefinition = ActionDefinition {
    id: ActionId::FOCUS_LAST_WORKSPACE,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::FocusLastWorkspace,
    category: ActionCategory::Workspace,
    title: "Focus last workspace",
    description: "Focus the previously focused workspace on the target monitor",
    keywords: &["workspace", "last", "focus"],
    parameters: NONE,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const CLOSE_WORKSPACE: ActionDefinition = ActionDefinition {
    id: ActionId::CLOSE_WORKSPACE,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::CloseWorkspace,
    category: ActionCategory::Workspace,
    title: "Close workspace",
    description: "Close the focused empty unnamed workspace",
    keywords: &["workspace", "close"],
    parameters: NONE,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const FOCUS_MONITOR: ActionDefinition = ActionDefinition {
    id: ActionId::FOCUS_MONITOR,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::FocusMonitor,
    category: ActionCategory::Workspace,
    title: "Focus monitor",
    description: "Focus a monitor by index",
    keywords: &["monitor", "focus"],
    parameters: INDEX,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const CYCLE_FOCUS_MONITOR: ActionDefinition = ActionDefinition {
    id: ActionId::CYCLE_FOCUS_MONITOR,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::CycleFocusMonitor,
    category: ActionCategory::Workspace,
    title: "Cycle focus monitor",
    description: "Focus the next or previous monitor",
    keywords: &["monitor", "focus", "cycle"],
    parameters: CYCLE,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const FOCUS_MONITOR_AT_CURSOR: ActionDefinition = ActionDefinition {
    id: ActionId::FOCUS_MONITOR_AT_CURSOR,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::FocusMonitorAtCursor,
    category: ActionCategory::Workspace,
    title: "Focus monitor at cursor",
    description: "Focus the monitor under the cursor",
    keywords: &["monitor", "cursor", "focus"],
    parameters: NONE,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const FOCUS_WORKSPACE_ON_ALL_MONITORS: ActionDefinition = ActionDefinition {
    id: ActionId::FOCUS_WORKSPACE_ON_ALL_MONITORS,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::FocusWorkspaceOnAllMonitors,
    category: ActionCategory::Workspace,
    title: "Focus workspace on all monitors",
    description: "Focus the same workspace index on every monitor",
    keywords: &["workspace", "monitors", "focus"],
    parameters: INDEX,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const FOCUS_MONITOR_WORKSPACE: ActionDefinition = ActionDefinition {
    id: ActionId::FOCUS_MONITOR_WORKSPACE,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::FocusMonitorWorkspace,
    category: ActionCategory::Workspace,
    title: "Focus monitor workspace",
    description: "Focus a workspace on a specific monitor",
    keywords: &["monitor", "workspace", "focus"],
    parameters: MONITOR_WORKSPACE,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const NEW_WORKSPACE: ActionDefinition = ActionDefinition {
    id: ActionId::NEW_WORKSPACE,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::NewWorkspace,
    category: ActionCategory::Workspace,
    title: "New workspace",
    description: "Create and focus a new workspace on the focused monitor",
    keywords: &["workspace", "new"],
    parameters: NONE,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const TOGGLE_TILING: ActionDefinition = ActionDefinition {
    id: ActionId::TOGGLE_TILING,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::ToggleTiling,
    category: ActionCategory::Workspace,
    title: "Toggle tiling",
    description: "Toggle tiling on the focused workspace",
    keywords: &["tiling", "toggle"],
    parameters: NONE,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const CYCLE_LAYOUT: ActionDefinition = ActionDefinition {
    id: ActionId::CYCLE_LAYOUT,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::CycleLayout,
    category: ActionCategory::Workspace,
    title: "Cycle layout",
    description: "Cycle the focused workspace layout",
    keywords: &["layout", "cycle"],
    parameters: CYCLE,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const FLIP_LAYOUT: ActionDefinition = ActionDefinition {
    id: ActionId::FLIP_LAYOUT,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::FlipLayout,
    category: ActionCategory::Workspace,
    title: "Flip layout",
    description: "Flip the focused workspace layout on an axis",
    keywords: &["layout", "flip"],
    parameters: AXIS,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const TOGGLE_WORKSPACE_LAYER: ActionDefinition = ActionDefinition {
    id: ActionId::TOGGLE_WORKSPACE_LAYER,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::ToggleWorkspaceLayer,
    category: ActionCategory::Workspace,
    title: "Toggle workspace layer",
    description: "Toggle between tiling and floating layers",
    keywords: &["layer", "tiling", "floating"],
    parameters: NONE,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const MOVE_CONTAINER_TO_LAST_WORKSPACE: ActionDefinition = ActionDefinition {
    id: ActionId::MOVE_CONTAINER_TO_LAST_WORKSPACE,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::MoveContainerToLastWorkspace,
    category: ActionCategory::Workspace,
    title: "Move container to last workspace",
    description: "Move the focused container to the last workspace and follow it",
    keywords: &["container", "move", "workspace"],
    parameters: NONE,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const SEND_CONTAINER_TO_LAST_WORKSPACE: ActionDefinition = ActionDefinition {
    id: ActionId::SEND_CONTAINER_TO_LAST_WORKSPACE,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::SendContainerToLastWorkspace,
    category: ActionCategory::Workspace,
    title: "Send container to last workspace",
    description: "Send the focused container to the last workspace without following",
    keywords: &["container", "send", "workspace"],
    parameters: NONE,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const MOVE_CONTAINER_TO_WORKSPACE: ActionDefinition = ActionDefinition {
    id: ActionId::MOVE_CONTAINER_TO_WORKSPACE,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::MoveContainerToWorkspace,
    category: ActionCategory::Workspace,
    title: "Move container to workspace",
    description: "Move the focused container to a workspace and follow it",
    keywords: &["container", "move", "workspace"],
    parameters: INDEX,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const CYCLE_MOVE_CONTAINER_TO_WORKSPACE: ActionDefinition = ActionDefinition {
    id: ActionId::CYCLE_MOVE_CONTAINER_TO_WORKSPACE,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::CycleMoveContainerToWorkspace,
    category: ActionCategory::Workspace,
    title: "Cycle move container to workspace",
    description: "Move the focused container to the next or previous workspace and follow it",
    keywords: &["container", "move", "cycle"],
    parameters: CYCLE,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const SEND_CONTAINER_TO_WORKSPACE: ActionDefinition = ActionDefinition {
    id: ActionId::SEND_CONTAINER_TO_WORKSPACE,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::SendContainerToWorkspace,
    category: ActionCategory::Workspace,
    title: "Send container to workspace",
    description: "Send the focused container to a workspace without following",
    keywords: &["container", "send", "workspace"],
    parameters: INDEX,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const CYCLE_SEND_CONTAINER_TO_WORKSPACE: ActionDefinition = ActionDefinition {
    id: ActionId::CYCLE_SEND_CONTAINER_TO_WORKSPACE,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::CycleSendContainerToWorkspace,
    category: ActionCategory::Workspace,
    title: "Cycle send container to workspace",
    description: "Send the focused container to the next or previous workspace without following",
    keywords: &["container", "send", "cycle"],
    parameters: CYCLE,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const MOVE_CONTAINER_TO_MONITOR: ActionDefinition = ActionDefinition {
    id: ActionId::MOVE_CONTAINER_TO_MONITOR,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::MoveContainerToMonitor,
    category: ActionCategory::Workspace,
    title: "Move container to monitor",
    description: "Move the focused container to a monitor and follow it",
    keywords: &["container", "move", "monitor"],
    parameters: INDEX,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const CYCLE_MOVE_CONTAINER_TO_MONITOR: ActionDefinition = ActionDefinition {
    id: ActionId::CYCLE_MOVE_CONTAINER_TO_MONITOR,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::CycleMoveContainerToMonitor,
    category: ActionCategory::Workspace,
    title: "Cycle move container to monitor",
    description: "Move the focused container to the next or previous monitor and follow it",
    keywords: &["container", "move", "monitor", "cycle"],
    parameters: CYCLE,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const SEND_CONTAINER_TO_MONITOR: ActionDefinition = ActionDefinition {
    id: ActionId::SEND_CONTAINER_TO_MONITOR,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::SendContainerToMonitor,
    category: ActionCategory::Workspace,
    title: "Send container to monitor",
    description: "Send the focused container to a monitor without following",
    keywords: &["container", "send", "monitor"],
    parameters: INDEX,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const CYCLE_SEND_CONTAINER_TO_MONITOR: ActionDefinition = ActionDefinition {
    id: ActionId::CYCLE_SEND_CONTAINER_TO_MONITOR,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::CycleSendContainerToMonitor,
    category: ActionCategory::Workspace,
    title: "Cycle send container to monitor",
    description: "Send the focused container to the next or previous monitor without following",
    keywords: &["container", "send", "monitor", "cycle"],
    parameters: CYCLE,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const MOVE_CONTAINER_TO_MONITOR_WORKSPACE: ActionDefinition = ActionDefinition {
    id: ActionId::MOVE_CONTAINER_TO_MONITOR_WORKSPACE,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::MoveContainerToMonitorWorkspace,
    category: ActionCategory::Workspace,
    title: "Move container to monitor workspace",
    description: "Move the focused container to a workspace on a monitor and follow it",
    keywords: &["container", "move", "monitor", "workspace"],
    parameters: MONITOR_WORKSPACE,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const SEND_CONTAINER_TO_MONITOR_WORKSPACE: ActionDefinition = ActionDefinition {
    id: ActionId::SEND_CONTAINER_TO_MONITOR_WORKSPACE,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::SendContainerToMonitorWorkspace,
    category: ActionCategory::Workspace,
    title: "Send container to monitor workspace",
    description: "Send the focused container to a workspace on a monitor without following",
    keywords: &["container", "send", "monitor", "workspace"],
    parameters: MONITOR_WORKSPACE,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const MOVE_WORKSPACE_TO_MONITOR: ActionDefinition = ActionDefinition {
    id: ActionId::MOVE_WORKSPACE_TO_MONITOR,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::MoveWorkspaceToMonitor,
    category: ActionCategory::Workspace,
    title: "Move workspace to monitor",
    description: "Move the focused workspace to a monitor",
    keywords: &["workspace", "move", "monitor"],
    parameters: INDEX,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const CYCLE_MOVE_WORKSPACE_TO_MONITOR: ActionDefinition = ActionDefinition {
    id: ActionId::CYCLE_MOVE_WORKSPACE_TO_MONITOR,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::CycleMoveWorkspaceToMonitor,
    category: ActionCategory::Workspace,
    title: "Cycle move workspace to monitor",
    description: "Move the focused workspace to the next or previous monitor",
    keywords: &["workspace", "move", "monitor", "cycle"],
    parameters: CYCLE,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const SWAP_WORKSPACES_TO_MONITOR: ActionDefinition = ActionDefinition {
    id: ActionId::SWAP_WORKSPACES_TO_MONITOR,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::SwapWorkspacesToMonitor,
    category: ActionCategory::Workspace,
    title: "Swap workspaces to monitor",
    description: "Swap the focused monitor's workspaces with another monitor",
    keywords: &["workspace", "swap", "monitor"],
    parameters: INDEX,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const RETILE: ActionDefinition = ActionDefinition {
    id: ActionId::RETILE,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::Retile,
    category: ActionCategory::Workspace,
    title: "Retile",
    description: "Retile every workspace",
    keywords: &["retile", "layout"],
    parameters: NONE,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const RETILE_WITH_RESIZE_DIMENSIONS: ActionDefinition = ActionDefinition {
    id: ActionId::RETILE_WITH_RESIZE_DIMENSIONS,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::RetileWithResizeDimensions,
    category: ActionCategory::Workspace,
    title: "Retile with resize dimensions",
    description: "Retile every workspace while preserving resize dimensions",
    keywords: &["retile", "resize"],
    parameters: NONE,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const ADJUST_CONTAINER_PADDING: ActionDefinition = ActionDefinition {
    id: ActionId::ADJUST_CONTAINER_PADDING,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::AdjustContainerPadding,
    category: ActionCategory::Workspace,
    title: "Adjust container padding",
    description: "Increase or decrease container padding on the focused workspace",
    keywords: &["padding", "container"],
    parameters: PADDING,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const ADJUST_WORKSPACE_PADDING: ActionDefinition = ActionDefinition {
    id: ActionId::ADJUST_WORKSPACE_PADDING,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::AdjustWorkspacePadding,
    category: ActionCategory::Workspace,
    title: "Adjust workspace padding",
    description: "Increase or decrease workspace padding on the focused workspace",
    keywords: &["padding", "workspace"],
    parameters: PADDING,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const TOGGLE_MOUSE_FOLLOWS_FOCUS: ActionDefinition = ActionDefinition {
    id: ActionId::TOGGLE_MOUSE_FOLLOWS_FOCUS,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::ToggleMouseFollowsFocus,
    category: ActionCategory::Workspace,
    title: "Toggle mouse follows focus",
    description: "Toggle whether the mouse follows focus changes",
    keywords: &["mouse", "focus"],
    parameters: NONE,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const SET_MOUSE_FOLLOWS_FOCUS: ActionDefinition = ActionDefinition {
    id: ActionId::SET_MOUSE_FOLLOWS_FOCUS,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::SetMouseFollowsFocus,
    category: ActionCategory::Workspace,
    title: "Set mouse follows focus",
    description: "Enable or disable mouse follows focus",
    keywords: &["mouse", "focus"],
    parameters: FLAG,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const TOGGLE_WORKSPACE_WINDOW_CONTAINER_BEHAVIOUR: ActionDefinition = ActionDefinition {
    id: ActionId::TOGGLE_WORKSPACE_WINDOW_CONTAINER_BEHAVIOUR,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::ToggleWorkspaceWindowContainerBehaviour,
    category: ActionCategory::Workspace,
    title: "Toggle workspace window container behaviour",
    description: "Toggle container behaviour on the focused workspace",
    keywords: &["workspace", "container", "behaviour"],
    parameters: NONE,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const TOGGLE_WORKSPACE_FLOAT_OVERRIDE: ActionDefinition = ActionDefinition {
    id: ActionId::TOGGLE_WORKSPACE_FLOAT_OVERRIDE,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::ToggleWorkspaceFloatOverride,
    category: ActionCategory::Workspace,
    title: "Toggle workspace float override",
    description: "Toggle float override on the focused workspace",
    keywords: &["workspace", "float", "override"],
    parameters: NONE,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const TOGGLE_CROSS_MONITOR_MOVE_BEHAVIOUR: ActionDefinition = ActionDefinition {
    id: ActionId::TOGGLE_CROSS_MONITOR_MOVE_BEHAVIOUR,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::ToggleCrossMonitorMoveBehaviour,
    category: ActionCategory::Workspace,
    title: "Toggle cross monitor move behaviour",
    description: "Toggle between swap and insert when moving across monitors",
    keywords: &["monitor", "move", "behaviour"],
    parameters: NONE,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const TOGGLE_PAUSE: ActionDefinition = ActionDefinition {
    id: ActionId::TOGGLE_PAUSE,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::TogglePause,
    category: ActionCategory::Workspace,
    title: "Toggle pause",
    description: "Pause or resume window management",
    keywords: &["pause", "resume"],
    parameters: NONE,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const SET_FOCUSED_CONTAINER_PADDING: ActionDefinition = ActionDefinition {
    id: ActionId::SET_FOCUSED_CONTAINER_PADDING,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::SetFocusedContainerPadding,
    category: ActionCategory::Workspace,
    title: "Set focused container padding",
    description: "Set container padding on the focused workspace",
    keywords: &["padding", "container"],
    parameters: SIZE,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const SET_FOCUSED_WORKSPACE_PADDING: ActionDefinition = ActionDefinition {
    id: ActionId::SET_FOCUSED_WORKSPACE_PADDING,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::SetFocusedWorkspacePadding,
    category: ActionCategory::Workspace,
    title: "Set focused workspace padding",
    description: "Set workspace padding on the focused workspace",
    keywords: &["padding", "workspace"],
    parameters: SIZE,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const SET_CONTAINER_PADDING: ActionDefinition = ActionDefinition {
    id: ActionId::SET_CONTAINER_PADDING,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::SetContainerPadding,
    category: ActionCategory::Workspace,
    title: "Set container padding",
    description: "Set container padding on a workspace",
    keywords: &["padding", "container"],
    parameters: MONITOR_WORKSPACE_SIZE,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const SET_WORKSPACE_PADDING: ActionDefinition = ActionDefinition {
    id: ActionId::SET_WORKSPACE_PADDING,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::SetWorkspacePadding,
    category: ActionCategory::Workspace,
    title: "Set workspace padding",
    description: "Set workspace padding on a workspace",
    keywords: &["padding", "workspace"],
    parameters: MONITOR_WORKSPACE_SIZE,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const SET_WORKSPACE_TILING: ActionDefinition = ActionDefinition {
    id: ActionId::SET_WORKSPACE_TILING,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::SetWorkspaceTiling,
    category: ActionCategory::Workspace,
    title: "Set workspace tiling",
    description: "Enable or disable tiling on a workspace",
    keywords: &["tiling", "workspace"],
    parameters: MONITOR_WORKSPACE_FLAG,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const SET_MONITOR_WORKSPACE_LAYOUT: ActionDefinition = ActionDefinition {
    id: ActionId::SET_MONITOR_WORKSPACE_LAYOUT,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::SetMonitorWorkspaceLayout,
    category: ActionCategory::Workspace,
    title: "Set monitor workspace layout",
    description: "Set the default layout on a workspace",
    keywords: &["layout", "workspace"],
    parameters: MONITOR_WORKSPACE_LAYOUT,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const ENSURE_WORKSPACES: ActionDefinition = ActionDefinition {
    id: ActionId::ENSURE_WORKSPACES,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::EnsureWorkspaces,
    category: ActionCategory::Workspace,
    title: "Ensure workspaces",
    description: "Ensure a monitor has at least a given number of workspaces",
    keywords: &["workspace", "ensure"],
    parameters: MONITOR_COUNT,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const CLEAR_WORKSPACE_LAYOUT_RULES: ActionDefinition = ActionDefinition {
    id: ActionId::CLEAR_WORKSPACE_LAYOUT_RULES,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::ClearWorkspaceLayoutRules,
    category: ActionCategory::Workspace,
    title: "Clear workspace layout rules",
    description: "Clear layout rules on a workspace",
    keywords: &["layout", "rules"],
    parameters: MONITOR_WORKSPACE,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const SET_SCROLLING_COLUMNS: ActionDefinition = ActionDefinition {
    id: ActionId::SET_SCROLLING_COLUMNS,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::SetScrollingColumns,
    category: ActionCategory::Workspace,
    title: "Set scrolling columns",
    description: "Set the scrolling layout column count",
    keywords: &["scrolling", "columns"],
    parameters: COLUMNS,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const ENFORCE_WORKSPACE_RULES: ActionDefinition = ActionDefinition {
    id: ActionId::ENFORCE_WORKSPACE_RULES,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::EnforceWorkspaceRules,
    category: ActionCategory::Workspace,
    title: "Enforce workspace rules",
    description: "Apply workspace matching rules now",
    keywords: &["rules", "workspace"],
    parameters: NONE,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const ADD_WORKSPACE_LAYOUT_RULE: ActionDefinition = def(
    ActionId::ADD_WORKSPACE_LAYOUT_RULE,
    BuiltinActionKind::AddWorkspaceLayoutRule,
    ActionCategory::Workspace,
    "Add workspace layout rule",
    "Apply a built-in layout when a workspace reaches a container count",
    &["layout", "rule"],
    LAYOUT_RULE,
);

pub const FOCUS_NAMED_WORKSPACE: ActionDefinition = def(
    ActionId::FOCUS_NAMED_WORKSPACE,
    BuiltinActionKind::FocusNamedWorkspace,
    ActionCategory::Workspace,
    "Focus named workspace",
    "Focus the workspace with this name",
    &["focus", "workspace", "name"],
    NAME,
);

pub const MOVE_CONTAINER_TO_NAMED_WORKSPACE: ActionDefinition = def(
    ActionId::MOVE_CONTAINER_TO_NAMED_WORKSPACE,
    BuiltinActionKind::MoveContainerToNamedWorkspace,
    ActionCategory::Workspace,
    "Move container to named workspace",
    "Move the focused container to the named workspace and follow it",
    &["move", "workspace", "name"],
    NAME,
);

pub const SEND_CONTAINER_TO_NAMED_WORKSPACE: ActionDefinition = def(
    ActionId::SEND_CONTAINER_TO_NAMED_WORKSPACE,
    BuiltinActionKind::SendContainerToNamedWorkspace,
    ActionCategory::Workspace,
    "Send container to named workspace",
    "Send the focused container to the named workspace",
    &["send", "workspace", "name"],
    NAME,
);

pub const SET_NAMED_WORKSPACE_CONTAINER_PADDING: ActionDefinition = def(
    ActionId::SET_NAMED_WORKSPACE_CONTAINER_PADDING,
    BuiltinActionKind::SetNamedWorkspaceContainerPadding,
    ActionCategory::Workspace,
    "Set named workspace container padding",
    "Set container padding on the named workspace",
    &["padding", "name"],
    NAME_SIZE,
);

pub const SET_NAMED_WORKSPACE_PADDING: ActionDefinition = def(
    ActionId::SET_NAMED_WORKSPACE_PADDING,
    BuiltinActionKind::SetNamedWorkspacePadding,
    ActionCategory::Workspace,
    "Set named workspace padding",
    "Set workspace padding on the named workspace",
    &["padding", "name"],
    NAME_SIZE,
);

pub const SET_NAMED_WORKSPACE_TILING: ActionDefinition = def(
    ActionId::SET_NAMED_WORKSPACE_TILING,
    BuiltinActionKind::SetNamedWorkspaceTiling,
    ActionCategory::Workspace,
    "Set named workspace tiling",
    "Enable or disable tiling on the named workspace",
    &["tiling", "name"],
    NAME_FLAG,
);

pub const SET_NAMED_WORKSPACE_LAYOUT: ActionDefinition = def(
    ActionId::SET_NAMED_WORKSPACE_LAYOUT,
    BuiltinActionKind::SetNamedWorkspaceLayout,
    ActionCategory::Workspace,
    "Set named workspace layout",
    "Set a built-in layout on the named workspace",
    &["layout", "name"],
    NAME_LAYOUT,
);

pub const SET_NAMED_WORKSPACE_CUSTOM_LAYOUT: ActionDefinition = def(
    ActionId::SET_NAMED_WORKSPACE_CUSTOM_LAYOUT,
    BuiltinActionKind::SetNamedWorkspaceCustomLayout,
    ActionCategory::Workspace,
    "Set named workspace custom layout",
    "Set a custom layout file on the named workspace",
    &["layout", "custom", "name"],
    NAME_PATH,
);

pub const ADD_NAMED_WORKSPACE_LAYOUT_RULE: ActionDefinition = def(
    ActionId::ADD_NAMED_WORKSPACE_LAYOUT_RULE,
    BuiltinActionKind::AddNamedWorkspaceLayoutRule,
    ActionCategory::Workspace,
    "Add named workspace layout rule",
    "Add a built-in layout rule to the named workspace",
    &["layout", "rule", "name"],
    NAME_LAYOUT_RULE,
);

pub const ADD_NAMED_WORKSPACE_CUSTOM_LAYOUT_RULE: ActionDefinition = def(
    ActionId::ADD_NAMED_WORKSPACE_CUSTOM_LAYOUT_RULE,
    BuiltinActionKind::AddNamedWorkspaceCustomLayoutRule,
    ActionCategory::Workspace,
    "Add named workspace custom layout rule",
    "Add a custom layout rule to the named workspace",
    &["layout", "rule", "custom", "name"],
    NAME_CUSTOM_LAYOUT_RULE,
);

pub const CLEAR_NAMED_WORKSPACE_LAYOUT_RULES: ActionDefinition = def(
    ActionId::CLEAR_NAMED_WORKSPACE_LAYOUT_RULES,
    BuiltinActionKind::ClearNamedWorkspaceLayoutRules,
    ActionCategory::Workspace,
    "Clear named workspace layout rules",
    "Clear layout rules on the named workspace",
    &["layout", "rule", "name"],
    NAME,
);

pub const ENSURE_NAMED_WORKSPACES: ActionDefinition = def(
    ActionId::ENSURE_NAMED_WORKSPACES,
    BuiltinActionKind::EnsureNamedWorkspaces,
    ActionCategory::Workspace,
    "Ensure named workspaces",
    "Ensure a monitor has workspaces with these names",
    &["workspace", "name"],
    MONITOR_NAMES,
);

pub const SET_WORKSPACE_NAME: ActionDefinition = def(
    ActionId::SET_WORKSPACE_NAME,
    BuiltinActionKind::SetWorkspaceName,
    ActionCategory::Workspace,
    "Set workspace name",
    "Name a workspace on a monitor",
    &["workspace", "name"],
    MONITOR_WORKSPACE_NAME,
);

pub const SET_LAYOUT_RATIOS: ActionDefinition = def(
    ActionId::SET_LAYOUT_RATIOS,
    BuiltinActionKind::SetLayoutRatios,
    ActionCategory::Workspace,
    "Set layout ratios",
    "Set column and row ratios on the focused workspace",
    &["layout", "ratio"],
    RATIOS,
);

pub const SET_CUSTOM_LAYOUT: ActionDefinition = def(
    ActionId::SET_CUSTOM_LAYOUT,
    BuiltinActionKind::SetCustomLayout,
    ActionCategory::Workspace,
    "Set custom layout",
    "Set a custom layout file on the focused workspace",
    &["layout", "custom"],
    PATH,
);

pub const SET_WORKSPACE_CUSTOM_LAYOUT: ActionDefinition = def(
    ActionId::SET_WORKSPACE_CUSTOM_LAYOUT,
    BuiltinActionKind::SetWorkspaceCustomLayout,
    ActionCategory::Workspace,
    "Set workspace custom layout",
    "Set a custom layout file on a monitor workspace",
    &["layout", "custom"],
    MONITOR_WORKSPACE_PATH,
);

pub const ADD_WORKSPACE_CUSTOM_LAYOUT_RULE: ActionDefinition = def(
    ActionId::ADD_WORKSPACE_CUSTOM_LAYOUT_RULE,
    BuiltinActionKind::AddWorkspaceCustomLayoutRule,
    ActionCategory::Workspace,
    "Add workspace custom layout rule",
    "Add a custom layout rule to a monitor workspace",
    &["layout", "rule", "custom"],
    CUSTOM_LAYOUT_RULE,
);
