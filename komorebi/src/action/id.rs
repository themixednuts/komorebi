use std::fmt;

use serde::Deserialize;
use serde::Serialize;
use uuid::Uuid;

pub use komorebi_protocol::InvocationId;
pub use komorebi_protocol::PrincipalId;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize)]
#[serde(transparent)]
pub struct ActionId(&'static str);

impl ActionId {
    pub const FOCUS_WINDOW: Self = Self("focus-window");
    pub const MOVE_WINDOW: Self = Self("move-window");
    pub const RESIZE_WINDOW: Self = Self("resize-window");
    pub const SET_WORKSPACE_LAYOUT: Self = Self("set-workspace-layout");
    pub const TOGGLE_WINDOW_FLOAT: Self = Self("toggle-window-float");
    pub const CYCLE_FOCUS_WINDOW: Self = Self("cycle-focus-window");
    pub const CYCLE_MOVE_WINDOW: Self = Self("cycle-move-window");
    pub const TOGGLE_WINDOW_MONOCLE: Self = Self("toggle-window-monocle");
    pub const TOGGLE_WINDOW_MAXIMIZE: Self = Self("toggle-window-maximize");
    pub const TOGGLE_CONTAINER_LOCK: Self = Self("toggle-container-lock");
    pub const STACK_WINDOW: Self = Self("stack-window");
    pub const UNSTACK_WINDOW: Self = Self("unstack-window");
    pub const STACK_ALL: Self = Self("stack-all");
    pub const UNSTACK_ALL: Self = Self("unstack-all");
    pub const CYCLE_STACK: Self = Self("cycle-stack");
    pub const CYCLE_STACK_INDEX: Self = Self("cycle-stack-index");
    pub const FOCUS_STACK_WINDOW: Self = Self("focus-stack-window");
    pub const FOCUS_WORKSPACE: Self = Self("focus-workspace");
    pub const CYCLE_FOCUS_WORKSPACE: Self = Self("cycle-focus-workspace");
    pub const CYCLE_FOCUS_EMPTY_WORKSPACE: Self = Self("cycle-focus-empty-workspace");
    pub const FOCUS_LAST_WORKSPACE: Self = Self("focus-last-workspace");
    pub const CLOSE_WORKSPACE: Self = Self("close-workspace");
    pub const FOCUS_MONITOR: Self = Self("focus-monitor");
    pub const CYCLE_FOCUS_MONITOR: Self = Self("cycle-focus-monitor");
    pub const FOCUS_MONITOR_AT_CURSOR: Self = Self("focus-monitor-at-cursor");
    pub const FOCUS_WORKSPACE_ON_ALL_MONITORS: Self = Self("focus-workspace-on-all-monitors");
    pub const FOCUS_MONITOR_WORKSPACE: Self = Self("focus-monitor-workspace");
    pub const CLOSE_WINDOW: Self = Self("close-window");
    pub const MINIMIZE_WINDOW: Self = Self("minimize-window");
    pub const FORCE_FOCUS: Self = Self("force-focus");
    pub const PROMOTE_CONTAINER: Self = Self("promote-container");
    pub const PROMOTE_CONTAINER_SWAP: Self = Self("promote-container-swap");
    pub const PROMOTE_FOCUS: Self = Self("promote-focus");
    pub const PROMOTE_WINDOW: Self = Self("promote-window");
    pub const NEW_WORKSPACE: Self = Self("new-workspace");
    pub const TOGGLE_TILING: Self = Self("toggle-tiling");
    pub const CYCLE_LAYOUT: Self = Self("cycle-layout");
    pub const FLIP_LAYOUT: Self = Self("flip-layout");
    pub const TOGGLE_WORKSPACE_LAYER: Self = Self("toggle-workspace-layer");
    pub const MOVE_CONTAINER_TO_LAST_WORKSPACE: Self = Self("move-container-to-last-workspace");
    pub const SEND_CONTAINER_TO_LAST_WORKSPACE: Self = Self("send-container-to-last-workspace");
    pub const MOVE_CONTAINER_TO_WORKSPACE: Self = Self("move-container-to-workspace");
    pub const CYCLE_MOVE_CONTAINER_TO_WORKSPACE: Self = Self("cycle-move-container-to-workspace");
    pub const SEND_CONTAINER_TO_WORKSPACE: Self = Self("send-container-to-workspace");
    pub const CYCLE_SEND_CONTAINER_TO_WORKSPACE: Self = Self("cycle-send-container-to-workspace");
    pub const MOVE_CONTAINER_TO_MONITOR: Self = Self("move-container-to-monitor");
    pub const CYCLE_MOVE_CONTAINER_TO_MONITOR: Self = Self("cycle-move-container-to-monitor");
    pub const SEND_CONTAINER_TO_MONITOR: Self = Self("send-container-to-monitor");
    pub const CYCLE_SEND_CONTAINER_TO_MONITOR: Self = Self("cycle-send-container-to-monitor");
    pub const MOVE_CONTAINER_TO_MONITOR_WORKSPACE: Self =
        Self("move-container-to-monitor-workspace");
    pub const SEND_CONTAINER_TO_MONITOR_WORKSPACE: Self =
        Self("send-container-to-monitor-workspace");
    pub const MOVE_WORKSPACE_TO_MONITOR: Self = Self("move-workspace-to-monitor");
    pub const CYCLE_MOVE_WORKSPACE_TO_MONITOR: Self = Self("cycle-move-workspace-to-monitor");
    pub const SWAP_WORKSPACES_TO_MONITOR: Self = Self("swap-workspaces-to-monitor");
    pub const PRESELECT_DIRECTION: Self = Self("preselect-direction");
    pub const CANCEL_PRESELECT: Self = Self("cancel-preselect");
    pub const RETILE: Self = Self("retile");
    pub const RETILE_WITH_RESIZE_DIMENSIONS: Self = Self("retile-with-resize-dimensions");
    pub const MANAGE_FOCUSED_WINDOW: Self = Self("manage-focused-window");
    pub const UNMANAGE_FOCUSED_WINDOW: Self = Self("unmanage-focused-window");
    pub const ADJUST_CONTAINER_PADDING: Self = Self("adjust-container-padding");
    pub const ADJUST_WORKSPACE_PADDING: Self = Self("adjust-workspace-padding");
    pub const TOGGLE_MOUSE_FOLLOWS_FOCUS: Self = Self("toggle-mouse-follows-focus");
    pub const SET_MOUSE_FOLLOWS_FOCUS: Self = Self("set-mouse-follows-focus");
    pub const TOGGLE_WINDOW_CONTAINER_BEHAVIOUR: Self = Self("toggle-window-container-behaviour");
    pub const TOGGLE_FLOAT_OVERRIDE: Self = Self("toggle-float-override");
    pub const TOGGLE_WORKSPACE_WINDOW_CONTAINER_BEHAVIOUR: Self =
        Self("toggle-workspace-window-container-behaviour");
    pub const TOGGLE_WORKSPACE_FLOAT_OVERRIDE: Self = Self("toggle-workspace-float-override");
    pub const TOGGLE_CROSS_MONITOR_MOVE_BEHAVIOUR: Self =
        Self("toggle-cross-monitor-move-behaviour");
    pub const TOGGLE_MONOCLE_FOCUS_BEHAVIOUR: Self = Self("toggle-monocle-focus-behaviour");
    pub const TOGGLE_PAUSE: Self = Self("toggle-pause");
    pub const SET_FOCUSED_CONTAINER_PADDING: Self = Self("set-focused-container-padding");
    pub const SET_FOCUSED_WORKSPACE_PADDING: Self = Self("set-focused-workspace-padding");
    pub const SET_CONTAINER_PADDING: Self = Self("set-container-padding");
    pub const SET_WORKSPACE_PADDING: Self = Self("set-workspace-padding");
    pub const SET_WORKSPACE_TILING: Self = Self("set-workspace-tiling");
    pub const SET_MONITOR_WORKSPACE_LAYOUT: Self = Self("set-monitor-workspace-layout");
    pub const ENSURE_WORKSPACES: Self = Self("ensure-workspaces");
    pub const CLEAR_WORKSPACE_LAYOUT_RULES: Self = Self("clear-workspace-layout-rules");
    pub const SET_SCROLLING_COLUMNS: Self = Self("set-scrolling-columns");
    pub const LOCK_CONTAINER: Self = Self("lock-container");
    pub const UNLOCK_CONTAINER: Self = Self("unlock-container");
    pub const TOGGLE_TITLE_BARS: Self = Self("toggle-title-bars");
    pub const ENFORCE_WORKSPACE_RULES: Self = Self("enforce-workspace-rules");
    pub const ADD_SESSION_FLOAT_RULE: Self = Self("add-session-float-rule");
    pub const CLEAR_SESSION_FLOAT_RULES: Self = Self("clear-session-float-rules");
    pub const RESIZE_WINDOW_EDGE: Self = Self("resize-window-edge");
    pub const SET_WINDOW_HIDING_BEHAVIOUR: Self = Self("set-window-hiding-behaviour");
    pub const SET_CROSS_MONITOR_MOVE_BEHAVIOUR: Self = Self("set-cross-monitor-move-behaviour");
    pub const SET_MONOCLE_FOCUS_BEHAVIOUR: Self = Self("set-monocle-focus-behaviour");
    pub const SET_UNMANAGED_WINDOW_OPERATION_BEHAVIOUR: Self =
        Self("set-unmanaged-window-operation-behaviour");
    pub const SET_FOCUS_FOLLOWS_MOUSE: Self = Self("set-focus-follows-mouse");
    pub const TOGGLE_FOCUS_FOLLOWS_MOUSE: Self = Self("toggle-focus-follows-mouse");
    pub const ADD_WORKSPACE_LAYOUT_RULE: Self = Self("add-workspace-layout-rule");
    pub const FOCUS_NAMED_WORKSPACE: Self = Self("focus-named-workspace");
    pub const MOVE_CONTAINER_TO_NAMED_WORKSPACE: Self = Self("move-container-to-named-workspace");
    pub const SEND_CONTAINER_TO_NAMED_WORKSPACE: Self = Self("send-container-to-named-workspace");
    pub const SET_NAMED_WORKSPACE_CONTAINER_PADDING: Self =
        Self("set-named-workspace-container-padding");
    pub const SET_NAMED_WORKSPACE_PADDING: Self = Self("set-named-workspace-padding");
    pub const SET_NAMED_WORKSPACE_TILING: Self = Self("set-named-workspace-tiling");
    pub const SET_NAMED_WORKSPACE_LAYOUT: Self = Self("set-named-workspace-layout");
    pub const SET_NAMED_WORKSPACE_CUSTOM_LAYOUT: Self = Self("set-named-workspace-custom-layout");
    pub const ADD_NAMED_WORKSPACE_LAYOUT_RULE: Self = Self("add-named-workspace-layout-rule");
    pub const ADD_NAMED_WORKSPACE_CUSTOM_LAYOUT_RULE: Self =
        Self("add-named-workspace-custom-layout-rule");
    pub const CLEAR_NAMED_WORKSPACE_LAYOUT_RULES: Self = Self("clear-named-workspace-layout-rules");
    pub const ENSURE_NAMED_WORKSPACES: Self = Self("ensure-named-workspaces");
    pub const SET_WORKSPACE_NAME: Self = Self("set-workspace-name");
    pub const SET_LAYOUT_RATIOS: Self = Self("set-layout-ratios");
    pub const SET_CUSTOM_LAYOUT: Self = Self("set-custom-layout");
    pub const SET_WORKSPACE_CUSTOM_LAYOUT: Self = Self("set-workspace-custom-layout");
    pub const ADD_WORKSPACE_CUSTOM_LAYOUT_RULE: Self = Self("add-workspace-custom-layout-rule");
    pub const EAGER_FOCUS: Self = Self("eager-focus");
    pub const REMOVE_TITLE_BAR: Self = Self("remove-title-bar");

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

impl fmt::Display for ActionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize)]
#[serde(transparent)]
pub struct ParameterId(&'static str);

impl ParameterId {
    pub const DIRECTION: Self = Self("direction");
    pub const AXIS: Self = Self("axis");
    pub const DELTA: Self = Self("delta");
    pub const WORKSPACE: Self = Self("workspace");
    pub const LAYOUT: Self = Self("layout");
    pub const WINDOW: Self = Self("window");
    pub const CYCLE: Self = Self("cycle");
    pub const INDEX: Self = Self("index");
    pub const MONITOR: Self = Self("monitor");
    pub const SIZING: Self = Self("sizing");
    pub const ADJUSTMENT: Self = Self("adjustment");
    pub const ENABLED: Self = Self("enabled");
    pub const SIZE: Self = Self("size");
    pub const COUNT: Self = Self("count");
    pub const CONTAINER: Self = Self("container");
    pub const COLUMNS: Self = Self("columns");
    pub const NAME: Self = Self("name");
    pub const PATH: Self = Self("path");
    pub const BEHAVIOUR: Self = Self("behaviour");
    pub const IMPLEMENTATION: Self = Self("implementation");
    pub const EXE: Self = Self("exe");
    pub const IDENTIFIER: Self = Self("identifier");
    pub const NAMES: Self = Self("names");
    pub const COLUMN_RATIOS: Self = Self("column-ratios");
    pub const ROW_RATIOS: Self = Self("row-ratios");
    pub const AT_COUNT: Self = Self("at-count");

    pub const ALL: [Self; 26] = [
        Self::DIRECTION,
        Self::AXIS,
        Self::DELTA,
        Self::WORKSPACE,
        Self::LAYOUT,
        Self::WINDOW,
        Self::CYCLE,
        Self::INDEX,
        Self::MONITOR,
        Self::SIZING,
        Self::ADJUSTMENT,
        Self::ENABLED,
        Self::SIZE,
        Self::COUNT,
        Self::CONTAINER,
        Self::COLUMNS,
        Self::NAME,
        Self::PATH,
        Self::BEHAVIOUR,
        Self::IMPLEMENTATION,
        Self::EXE,
        Self::IDENTIFIER,
        Self::NAMES,
        Self::COLUMN_RATIOS,
        Self::ROW_RATIOS,
        Self::AT_COUNT,
    ];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

impl fmt::Display for ParameterId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ActionSchemaVersion(u16);

impl ActionSchemaVersion {
    pub const V1: Self = Self(1);

    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WindowId(u64);

impl WindowId {
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }
}

#[derive(Clone, Copy, Eq, PartialEq, Hash)]
pub struct ConfirmationToken([u8; 16]);

impl ConfirmationToken {
    #[must_use]
    pub fn issue() -> Self {
        Self(*Uuid::new_v4().as_bytes())
    }

    #[must_use]
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

impl fmt::Debug for ConfirmationToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ConfirmationToken([redacted])")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UndoToken(Uuid);

impl UndoToken {
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for UndoToken {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirmation_token_does_not_print_its_secret() {
        let token = ConfirmationToken::from_bytes([7; 16]);
        assert_eq!(format!("{token:?}"), "ConfirmationToken([redacted])");
    }

    #[test]
    fn action_ids_are_stable_leaves() {
        assert_eq!(ActionId::FOCUS_WINDOW.as_str(), "focus-window");
        assert_eq!(ActionId::MOVE_WINDOW.as_str(), "move-window");
        assert_eq!(ActionId::RESIZE_WINDOW.as_str(), "resize-window");
        assert_eq!(
            ActionId::SET_WORKSPACE_LAYOUT.as_str(),
            "set-workspace-layout"
        );
        assert_eq!(
            ActionId::TOGGLE_WINDOW_FLOAT.as_str(),
            "toggle-window-float"
        );
        assert_eq!(ActionId::CYCLE_FOCUS_WINDOW.as_str(), "cycle-focus-window");
        assert_eq!(ActionId::CYCLE_MOVE_WINDOW.as_str(), "cycle-move-window");
        assert_eq!(
            ActionId::TOGGLE_WINDOW_MONOCLE.as_str(),
            "toggle-window-monocle"
        );
        assert_eq!(
            ActionId::TOGGLE_WINDOW_MAXIMIZE.as_str(),
            "toggle-window-maximize"
        );
        assert_eq!(
            ActionId::TOGGLE_CONTAINER_LOCK.as_str(),
            "toggle-container-lock"
        );
        assert_eq!(ActionId::STACK_WINDOW.as_str(), "stack-window");
        assert_eq!(ActionId::UNSTACK_WINDOW.as_str(), "unstack-window");
        assert_eq!(ActionId::STACK_ALL.as_str(), "stack-all");
        assert_eq!(ActionId::UNSTACK_ALL.as_str(), "unstack-all");
        assert_eq!(ActionId::CYCLE_STACK.as_str(), "cycle-stack");
        assert_eq!(ActionId::CYCLE_STACK_INDEX.as_str(), "cycle-stack-index");
        assert_eq!(ActionId::FOCUS_STACK_WINDOW.as_str(), "focus-stack-window");
        assert_eq!(ActionId::FOCUS_WORKSPACE.as_str(), "focus-workspace");
        assert_eq!(
            ActionId::CYCLE_FOCUS_WORKSPACE.as_str(),
            "cycle-focus-workspace"
        );
        assert_eq!(
            ActionId::CYCLE_FOCUS_EMPTY_WORKSPACE.as_str(),
            "cycle-focus-empty-workspace"
        );
        assert_eq!(
            ActionId::FOCUS_LAST_WORKSPACE.as_str(),
            "focus-last-workspace"
        );
        assert_eq!(ActionId::CLOSE_WORKSPACE.as_str(), "close-workspace");
        assert_eq!(ActionId::FOCUS_MONITOR.as_str(), "focus-monitor");
        assert_eq!(
            ActionId::CYCLE_FOCUS_MONITOR.as_str(),
            "cycle-focus-monitor"
        );
        assert_eq!(
            ActionId::FOCUS_MONITOR_AT_CURSOR.as_str(),
            "focus-monitor-at-cursor"
        );
        assert_eq!(
            ActionId::FOCUS_WORKSPACE_ON_ALL_MONITORS.as_str(),
            "focus-workspace-on-all-monitors"
        );
        assert_eq!(
            ActionId::FOCUS_MONITOR_WORKSPACE.as_str(),
            "focus-monitor-workspace"
        );
        assert_eq!(ActionId::CLOSE_WINDOW.as_str(), "close-window");
        assert_eq!(ActionId::MINIMIZE_WINDOW.as_str(), "minimize-window");
        assert_eq!(ActionId::FORCE_FOCUS.as_str(), "force-focus");
        assert_eq!(ActionId::PROMOTE_CONTAINER.as_str(), "promote-container");
        assert_eq!(
            ActionId::PROMOTE_CONTAINER_SWAP.as_str(),
            "promote-container-swap"
        );
        assert_eq!(ActionId::PROMOTE_FOCUS.as_str(), "promote-focus");
        assert_eq!(ActionId::PROMOTE_WINDOW.as_str(), "promote-window");
        assert_eq!(ActionId::NEW_WORKSPACE.as_str(), "new-workspace");
        assert_eq!(ActionId::TOGGLE_TILING.as_str(), "toggle-tiling");
        assert_eq!(ActionId::CYCLE_LAYOUT.as_str(), "cycle-layout");
        assert_eq!(ActionId::FLIP_LAYOUT.as_str(), "flip-layout");
        assert_eq!(
            ActionId::TOGGLE_WORKSPACE_LAYER.as_str(),
            "toggle-workspace-layer"
        );
    }

    #[test]
    fn parameter_ids_match_the_protocol_vocabulary() {
        let internal = ParameterId::ALL
            .iter()
            .map(|id| id.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        let protocol = komorebi_protocol::BuiltInParameterId::ALL
            .iter()
            .map(|id| id.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(internal, protocol);
    }
}
