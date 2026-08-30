use super::*;

pub const FOCUS_WINDOW: ActionDefinition = ActionDefinition {
    id: ActionId::FOCUS_WINDOW,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::FocusWindow,
    category: ActionCategory::Window,
    title: "Focus window",
    description: "Focus the neighboring window in one direction",
    keywords: &["focus", "window", "direction"],
    parameters: DIRECTION,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const MOVE_WINDOW: ActionDefinition = ActionDefinition {
    id: ActionId::MOVE_WINDOW,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::MoveWindow,
    category: ActionCategory::Window,
    title: "Move window",
    description: "Move the focused window in one direction",
    keywords: &["move", "window", "direction"],
    parameters: DIRECTION,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const RESIZE_WINDOW: ActionDefinition = ActionDefinition {
    id: ActionId::RESIZE_WINDOW,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::ResizeWindow,
    category: ActionCategory::Window,
    title: "Resize window",
    description: "Resize the focused window along one axis",
    keywords: &["resize", "window", "axis"],
    parameters: RESIZE,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const RESIZE_WINDOW_BY_STEP: ActionDefinition = ActionDefinition {
    id: ActionId::RESIZE_WINDOW_BY_STEP,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::ResizeWindowByStep,
    category: ActionCategory::Window,
    title: "Resize window by configured step",
    description: "Resize the focused window along one axis by the configured step",
    keywords: &["resize", "window", "axis", "step"],
    parameters: RESIZE_BY_STEP,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const TOGGLE_WINDOW_FLOAT: ActionDefinition = ActionDefinition {
    id: ActionId::TOGGLE_WINDOW_FLOAT,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::ToggleWindowFloat,
    category: ActionCategory::Window,
    title: "Toggle window float",
    description: "Toggle whether the focused window floats",
    keywords: &["float", "window", "toggle"],
    parameters: WINDOW,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const CYCLE_FOCUS_WINDOW: ActionDefinition = ActionDefinition {
    id: ActionId::CYCLE_FOCUS_WINDOW,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::CycleFocusWindow,
    category: ActionCategory::Window,
    title: "Cycle focus window",
    description: "Focus the next or previous window in the focused workspace",
    keywords: &["focus", "window", "cycle"],
    parameters: CYCLE,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const CYCLE_MOVE_WINDOW: ActionDefinition = ActionDefinition {
    id: ActionId::CYCLE_MOVE_WINDOW,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::CycleMoveWindow,
    category: ActionCategory::Window,
    title: "Cycle move window",
    description: "Move the focused window to the next or previous container",
    keywords: &["move", "window", "cycle"],
    parameters: CYCLE,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const TOGGLE_WINDOW_MONOCLE: ActionDefinition = ActionDefinition {
    id: ActionId::TOGGLE_WINDOW_MONOCLE,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::ToggleWindowMonocle,
    category: ActionCategory::Window,
    title: "Toggle window monocle",
    description: "Toggle monocle for the focused window",
    keywords: &["monocle", "window", "toggle"],
    parameters: WINDOW,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const TOGGLE_WINDOW_MAXIMIZE: ActionDefinition = ActionDefinition {
    id: ActionId::TOGGLE_WINDOW_MAXIMIZE,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::ToggleWindowMaximize,
    category: ActionCategory::Window,
    title: "Toggle window maximize",
    description: "Toggle maximize for the focused window",
    keywords: &["maximize", "window", "toggle"],
    parameters: WINDOW,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const TOGGLE_CONTAINER_LOCK: ActionDefinition = ActionDefinition {
    id: ActionId::TOGGLE_CONTAINER_LOCK,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::ToggleContainerLock,
    category: ActionCategory::Window,
    title: "Toggle container lock",
    description: "Toggle whether the focused container is locked",
    keywords: &["lock", "container", "toggle"],
    parameters: WINDOW,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const STACK_WINDOW: ActionDefinition = ActionDefinition {
    id: ActionId::STACK_WINDOW,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::StackWindow,
    category: ActionCategory::Window,
    title: "Stack window",
    description: "Stack the focused window onto the neighbor in one direction",
    keywords: &["stack", "window"],
    parameters: DIRECTION,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const UNSTACK_WINDOW: ActionDefinition = ActionDefinition {
    id: ActionId::UNSTACK_WINDOW,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::UnstackWindow,
    category: ActionCategory::Window,
    title: "Unstack window",
    description: "Remove the focused window from its stack",
    keywords: &["unstack", "window"],
    parameters: WINDOW,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const STACK_ALL: ActionDefinition = ActionDefinition {
    id: ActionId::STACK_ALL,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::StackAll,
    category: ActionCategory::Window,
    title: "Stack all",
    description: "Stack every window on the focused workspace",
    keywords: &["stack", "all"],
    parameters: NONE,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const UNSTACK_ALL: ActionDefinition = ActionDefinition {
    id: ActionId::UNSTACK_ALL,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::UnstackAll,
    category: ActionCategory::Window,
    title: "Unstack all",
    description: "Unstack every window on the focused workspace",
    keywords: &["unstack", "all"],
    parameters: NONE,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const CYCLE_STACK: ActionDefinition = ActionDefinition {
    id: ActionId::CYCLE_STACK,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::CycleStack,
    category: ActionCategory::Window,
    title: "Cycle stack",
    description: "Cycle the focused stack window",
    keywords: &["stack", "cycle"],
    parameters: CYCLE,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const CYCLE_STACK_INDEX: ActionDefinition = ActionDefinition {
    id: ActionId::CYCLE_STACK_INDEX,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::CycleStackIndex,
    category: ActionCategory::Window,
    title: "Cycle stack index",
    description: "Cycle the focused stack index",
    keywords: &["stack", "index", "cycle"],
    parameters: CYCLE,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const FOCUS_STACK_WINDOW: ActionDefinition = ActionDefinition {
    id: ActionId::FOCUS_STACK_WINDOW,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::FocusStackWindow,
    category: ActionCategory::Window,
    title: "Focus stack window",
    description: "Focus a window in the focused stack by index",
    keywords: &["stack", "focus", "index"],
    parameters: INDEX,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const CLOSE_WINDOW: ActionDefinition = ActionDefinition {
    id: ActionId::CLOSE_WINDOW,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::CloseWindow,
    category: ActionCategory::Window,
    title: "Close window",
    description: "Close the foreground window",
    keywords: &["close", "window"],
    parameters: WINDOW,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const MINIMIZE_WINDOW: ActionDefinition = ActionDefinition {
    id: ActionId::MINIMIZE_WINDOW,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::MinimizeWindow,
    category: ActionCategory::Window,
    title: "Minimize window",
    description: "Minimize the foreground window",
    keywords: &["minimize", "window"],
    parameters: WINDOW,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const FORCE_FOCUS: ActionDefinition = ActionDefinition {
    id: ActionId::FORCE_FOCUS,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::ForceFocus,
    category: ActionCategory::Window,
    title: "Force focus",
    description: "Force focus the focused window with a click",
    keywords: &["focus", "force"],
    parameters: WINDOW,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const PROMOTE_CONTAINER: ActionDefinition = ActionDefinition {
    id: ActionId::PROMOTE_CONTAINER,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::PromoteContainer,
    category: ActionCategory::Window,
    title: "Promote container",
    description: "Promote the focused container to the front of the layout",
    keywords: &["promote", "container"],
    parameters: NONE,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const PROMOTE_CONTAINER_SWAP: ActionDefinition = ActionDefinition {
    id: ActionId::PROMOTE_CONTAINER_SWAP,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::PromoteContainerSwap,
    category: ActionCategory::Window,
    title: "Promote container swap",
    description: "Swap the focused container with the front of the layout",
    keywords: &["promote", "swap"],
    parameters: NONE,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const PROMOTE_FOCUS: ActionDefinition = ActionDefinition {
    id: ActionId::PROMOTE_FOCUS,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::PromoteFocus,
    category: ActionCategory::Window,
    title: "Promote focus",
    description: "Move focus to the front container",
    keywords: &["promote", "focus"],
    parameters: NONE,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const PROMOTE_WINDOW: ActionDefinition = ActionDefinition {
    id: ActionId::PROMOTE_WINDOW,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::PromoteWindow,
    category: ActionCategory::Window,
    title: "Promote window",
    description: "Focus a neighbor and promote it to the front",
    keywords: &["promote", "window", "direction"],
    parameters: DIRECTION,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const PRESELECT_DIRECTION: ActionDefinition = ActionDefinition {
    id: ActionId::PRESELECT_DIRECTION,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::PreselectDirection,
    category: ActionCategory::Window,
    title: "Preselect direction",
    description: "Preselect a direction for the next container insertion",
    keywords: &["preselect", "direction"],
    parameters: DIRECTION,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const CANCEL_PRESELECT: ActionDefinition = ActionDefinition {
    id: ActionId::CANCEL_PRESELECT,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::CancelPreselect,
    category: ActionCategory::Window,
    title: "Cancel preselect",
    description: "Clear the workspace direction preselect",
    keywords: &["preselect", "cancel"],
    parameters: NONE,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const MANAGE_FOCUSED_WINDOW: ActionDefinition = ActionDefinition {
    id: ActionId::MANAGE_FOCUSED_WINDOW,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::ManageFocusedWindow,
    category: ActionCategory::Window,
    title: "Manage focused window",
    description: "Start managing the focused window",
    keywords: &["manage", "window"],
    parameters: NONE,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const UNMANAGE_FOCUSED_WINDOW: ActionDefinition = ActionDefinition {
    id: ActionId::UNMANAGE_FOCUSED_WINDOW,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::UnmanageFocusedWindow,
    category: ActionCategory::Window,
    title: "Unmanage focused window",
    description: "Stop managing the focused window",
    keywords: &["unmanage", "window"],
    parameters: NONE,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const TOGGLE_WINDOW_CONTAINER_BEHAVIOUR: ActionDefinition = ActionDefinition {
    id: ActionId::TOGGLE_WINDOW_CONTAINER_BEHAVIOUR,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::ToggleWindowContainerBehaviour,
    category: ActionCategory::Window,
    title: "Toggle window container behaviour",
    description: "Toggle between creating and appending window containers",
    keywords: &["container", "behaviour"],
    parameters: NONE,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const TOGGLE_FLOAT_OVERRIDE: ActionDefinition = ActionDefinition {
    id: ActionId::TOGGLE_FLOAT_OVERRIDE,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::ToggleFloatOverride,
    category: ActionCategory::Window,
    title: "Toggle float override",
    description: "Toggle the global float override",
    keywords: &["float", "override"],
    parameters: NONE,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const TOGGLE_MONOCLE_FOCUS_BEHAVIOUR: ActionDefinition = ActionDefinition {
    id: ActionId::TOGGLE_MONOCLE_FOCUS_BEHAVIOUR,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::ToggleMonocleFocusBehaviour,
    category: ActionCategory::Window,
    title: "Toggle monocle focus behaviour",
    description: "Toggle whether directional focus cycles through a monocle",
    keywords: &["monocle", "focus"],
    parameters: NONE,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const LOCK_CONTAINER: ActionDefinition = ActionDefinition {
    id: ActionId::LOCK_CONTAINER,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::LockContainer,
    category: ActionCategory::Window,
    title: "Lock container",
    description: "Lock a container on a workspace",
    keywords: &["lock", "container"],
    parameters: MONITOR_WORKSPACE_CONTAINER,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const UNLOCK_CONTAINER: ActionDefinition = ActionDefinition {
    id: ActionId::UNLOCK_CONTAINER,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::UnlockContainer,
    category: ActionCategory::Window,
    title: "Unlock container",
    description: "Unlock a container on a workspace",
    keywords: &["unlock", "container"],
    parameters: MONITOR_WORKSPACE_CONTAINER,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const TOGGLE_TITLE_BARS: ActionDefinition = ActionDefinition {
    id: ActionId::TOGGLE_TITLE_BARS,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::ToggleTitleBars,
    category: ActionCategory::Window,
    title: "Toggle title bars",
    description: "Toggle removal of title bars on managed windows",
    keywords: &["titlebar"],
    parameters: NONE,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const ADD_SESSION_FLOAT_RULE: ActionDefinition = ActionDefinition {
    id: ActionId::ADD_SESSION_FLOAT_RULE,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::AddSessionFloatRule,
    category: ActionCategory::Window,
    title: "Add session float rule",
    description: "Float the foreground window for this session",
    keywords: &["float", "session"],
    parameters: NONE,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const CLEAR_SESSION_FLOAT_RULES: ActionDefinition = ActionDefinition {
    id: ActionId::CLEAR_SESSION_FLOAT_RULES,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::ClearSessionFloatRules,
    category: ActionCategory::Window,
    title: "Clear session float rules",
    description: "Clear float rules created this session",
    keywords: &["float", "session"],
    parameters: NONE,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const RESIZE_WINDOW_EDGE: ActionDefinition = def(
    ActionId::RESIZE_WINDOW_EDGE,
    BuiltinActionKind::ResizeWindowEdge,
    ActionCategory::Window,
    "Resize window edge",
    "Resize the focused window from one edge",
    &["resize", "edge"],
    RESIZE_EDGE,
);

pub const RESIZE_WINDOW_EDGE_BY_STEP: ActionDefinition = def(
    ActionId::RESIZE_WINDOW_EDGE_BY_STEP,
    BuiltinActionKind::ResizeWindowEdgeByStep,
    ActionCategory::Window,
    "Resize window edge by configured step",
    "Resize the focused window from one edge by the configured step",
    &["resize", "edge", "step"],
    RESIZE_EDGE_BY_STEP,
);

pub const SET_WINDOW_HIDING_BEHAVIOUR: ActionDefinition = def(
    ActionId::SET_WINDOW_HIDING_BEHAVIOUR,
    BuiltinActionKind::SetWindowHidingBehaviour,
    ActionCategory::Window,
    "Set window hiding behaviour",
    "Set how windows are hidden when leaving a workspace",
    &["hide", "cloak"],
    BEHAVIOUR,
);

pub const SET_CROSS_MONITOR_MOVE_BEHAVIOUR: ActionDefinition = def(
    ActionId::SET_CROSS_MONITOR_MOVE_BEHAVIOUR,
    BuiltinActionKind::SetCrossMonitorMoveBehaviour,
    ActionCategory::Window,
    "Set cross-monitor move behaviour",
    "Set how window moves behave across monitors",
    &["monitor", "move"],
    BEHAVIOUR,
);

pub const SET_MONOCLE_FOCUS_BEHAVIOUR: ActionDefinition = def(
    ActionId::SET_MONOCLE_FOCUS_BEHAVIOUR,
    BuiltinActionKind::SetMonocleFocusBehaviour,
    ActionCategory::Window,
    "Set monocle focus behaviour",
    "Set how directional focus behaves while monocle is active",
    &["monocle", "focus"],
    BEHAVIOUR,
);

pub const SET_UNMANAGED_WINDOW_OPERATION_BEHAVIOUR: ActionDefinition = def(
    ActionId::SET_UNMANAGED_WINDOW_OPERATION_BEHAVIOUR,
    BuiltinActionKind::SetUnmanagedWindowOperationBehaviour,
    ActionCategory::Window,
    "Set unmanaged window operation behaviour",
    "Set whether commands apply to unmanaged and floating windows",
    &["unmanaged", "float"],
    BEHAVIOUR,
);

pub const SET_FOCUS_FOLLOWS_MOUSE: ActionDefinition = def(
    ActionId::SET_FOCUS_FOLLOWS_MOUSE,
    BuiltinActionKind::SetFocusFollowsMouse,
    ActionCategory::Window,
    "Set focus follows mouse",
    "Enable or disable a focus-follows-mouse implementation",
    &["focus", "mouse"],
    IMPLEMENTATION_FLAG,
);

pub const TOGGLE_FOCUS_FOLLOWS_MOUSE: ActionDefinition = def(
    ActionId::TOGGLE_FOCUS_FOLLOWS_MOUSE,
    BuiltinActionKind::ToggleFocusFollowsMouse,
    ActionCategory::Window,
    "Toggle focus follows mouse",
    "Toggle a focus-follows-mouse implementation",
    &["focus", "mouse"],
    IMPLEMENTATION,
);

pub const EAGER_FOCUS: ActionDefinition = def(
    ActionId::EAGER_FOCUS,
    BuiltinActionKind::EagerFocus,
    ActionCategory::Window,
    "Eager focus",
    "Focus a managed window by executable name",
    &["focus", "exe"],
    EXE,
);

pub const REMOVE_TITLE_BAR: ActionDefinition = def(
    ActionId::REMOVE_TITLE_BAR,
    BuiltinActionKind::RemoveTitleBar,
    ActionCategory::Window,
    "Remove title bar",
    "Remove the title bar from matching windows",
    &["title", "bar"],
    TITLE_BAR,
);
