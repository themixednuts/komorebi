use crate::core::DefaultLayout;

use super::builtin::BuiltinActionKind;
use super::id::ActionId;
use super::id::ActionSchemaVersion;
use super::id::ParameterId;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActionCategory {
    Window,
    Workspace,
    Configuration,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PermittedUse {
    Interactive,
    Automation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfirmationPolicy {
    None,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UndoPolicy {
    None,
    PriorManagerIntent,
    ExactCapturedState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParameterDomain {
    Direction,
    Axis,
    Pixels,
    WorkspaceSelector,
    WindowSelector,
    Layout,
    Cycle,
    Index,
    Sizing,
    Adjustment,
    Flag,
    Size,
    Count,
    Columns,
    Name,
    Path,
    Behaviour,
    Implementation,
    Exe,
    Identifier,
    Ratios,
    AtCount,
    ResizeStep,
    Alpha,
    WindowKind,
    ColourChannel,
    BorderWidth,
    BorderOffset,
    BorderStyle,
    BorderImplementation,
    StackbarMode,
    StackbarLabel,
    StackbarHeight,
    StackbarTabWidth,
    StackbarFontSize,
    StackbarFontFamily,
    AnimationPrefix,
    AnimationDuration,
    AnimationFps,
    AnimationStyle,
    WorkAreaOffset,
    CursorWarpPolicy,
    WorkspaceTarget,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArgumentCardinality {
    RequiredScalar,
    RequiredList,
    OptionalScalar,
    OptionalList,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParameterDefinition {
    pub id: ParameterId,
    pub domain: ParameterDomain,
    pub cardinality: ArgumentCardinality,
}

impl ParameterDefinition {
    #[must_use]
    pub const fn cardinality(self) -> ArgumentCardinality {
        self.cardinality
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ActionDefinition {
    pub id: ActionId,
    pub schema_version: ActionSchemaVersion,
    pub kind: BuiltinActionKind,
    pub category: ActionCategory,
    pub title: &'static str,
    pub description: &'static str,
    pub keywords: &'static [&'static str],
    pub parameters: &'static [ParameterDefinition],
    pub permitted_uses: &'static [PermittedUse],
    pub confirmation: ConfirmationPolicy,
    pub undo: UndoPolicy,
}

const BOTH_USES: &[PermittedUse] = &[PermittedUse::Interactive, PermittedUse::Automation];

const DIRECTION: &[ParameterDefinition] = &[ParameterDefinition {
    id: ParameterId::DIRECTION,
    domain: ParameterDomain::Direction,
    cardinality: ArgumentCardinality::RequiredScalar,
}];

const RESIZE: &[ParameterDefinition] = &[
    ParameterDefinition {
        id: ParameterId::AXIS,
        domain: ParameterDomain::Axis,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
    ParameterDefinition {
        id: ParameterId::DELTA,
        domain: ParameterDomain::Pixels,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
];

const RESIZE_BY_STEP: &[ParameterDefinition] = &[
    ParameterDefinition {
        id: ParameterId::AXIS,
        domain: ParameterDomain::Axis,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
    ParameterDefinition {
        id: ParameterId::SIZING,
        domain: ParameterDomain::Sizing,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
];

const LAYOUT: &[ParameterDefinition] = &[
    ParameterDefinition {
        id: ParameterId::WORKSPACE,
        domain: ParameterDomain::WorkspaceSelector,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
    ParameterDefinition {
        id: ParameterId::LAYOUT,
        domain: ParameterDomain::Layout,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
];

const WINDOW: &[ParameterDefinition] = &[ParameterDefinition {
    id: ParameterId::WINDOW,
    domain: ParameterDomain::WindowSelector,
    cardinality: ArgumentCardinality::RequiredScalar,
}];

const CYCLE: &[ParameterDefinition] = &[ParameterDefinition {
    id: ParameterId::CYCLE,
    domain: ParameterDomain::Cycle,
    cardinality: ArgumentCardinality::RequiredScalar,
}];

const INDEX: &[ParameterDefinition] = &[ParameterDefinition {
    id: ParameterId::INDEX,
    domain: ParameterDomain::Index,
    cardinality: ArgumentCardinality::RequiredScalar,
}];

const INDEX_CURSOR_WARP: &[ParameterDefinition] = &[
    ParameterDefinition {
        id: ParameterId::INDEX,
        domain: ParameterDomain::Index,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
    ParameterDefinition {
        id: ParameterId::CURSOR_WARP,
        domain: ParameterDomain::CursorWarpPolicy,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
];

const WORKSPACE_TARGET_CURSOR_WARP: &[ParameterDefinition] = &[
    ParameterDefinition {
        id: ParameterId::WORKSPACE_TARGET,
        domain: ParameterDomain::WorkspaceTarget,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
    ParameterDefinition {
        id: ParameterId::CURSOR_WARP,
        domain: ParameterDomain::CursorWarpPolicy,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
];

const MONITOR_WORKSPACE: &[ParameterDefinition] = &[
    ParameterDefinition {
        id: ParameterId::MONITOR,
        domain: ParameterDomain::Index,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
    ParameterDefinition {
        id: ParameterId::INDEX,
        domain: ParameterDomain::Index,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
];

const MONITOR_WORKSPACE_CURSOR_WARP: &[ParameterDefinition] = &[
    ParameterDefinition {
        id: ParameterId::MONITOR,
        domain: ParameterDomain::Index,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
    ParameterDefinition {
        id: ParameterId::INDEX,
        domain: ParameterDomain::Index,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
    ParameterDefinition {
        id: ParameterId::CURSOR_WARP,
        domain: ParameterDomain::CursorWarpPolicy,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
];

const NONE: &[ParameterDefinition] = &[];

const AXIS: &[ParameterDefinition] = &[ParameterDefinition {
    id: ParameterId::AXIS,
    domain: ParameterDomain::Axis,
    cardinality: ArgumentCardinality::RequiredScalar,
}];

const PADDING: &[ParameterDefinition] = &[
    ParameterDefinition {
        id: ParameterId::SIZING,
        domain: ParameterDomain::Sizing,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
    ParameterDefinition {
        id: ParameterId::ADJUSTMENT,
        domain: ParameterDomain::Adjustment,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
];

const FLAG: &[ParameterDefinition] = &[ParameterDefinition {
    id: ParameterId::ENABLED,
    domain: ParameterDomain::Flag,
    cardinality: ArgumentCardinality::RequiredScalar,
}];

const SIZE: &[ParameterDefinition] = &[ParameterDefinition {
    id: ParameterId::SIZE,
    domain: ParameterDomain::Size,
    cardinality: ArgumentCardinality::RequiredScalar,
}];

const COLUMNS: &[ParameterDefinition] = &[ParameterDefinition {
    id: ParameterId::COLUMNS,
    domain: ParameterDomain::Columns,
    cardinality: ArgumentCardinality::RequiredScalar,
}];

const MONITOR_COUNT: &[ParameterDefinition] = &[
    ParameterDefinition {
        id: ParameterId::MONITOR,
        domain: ParameterDomain::Index,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
    ParameterDefinition {
        id: ParameterId::COUNT,
        domain: ParameterDomain::Count,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
];

const MONITOR_WORKSPACE_SIZE: &[ParameterDefinition] = &[
    ParameterDefinition {
        id: ParameterId::MONITOR,
        domain: ParameterDomain::Index,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
    ParameterDefinition {
        id: ParameterId::INDEX,
        domain: ParameterDomain::Index,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
    ParameterDefinition {
        id: ParameterId::SIZE,
        domain: ParameterDomain::Size,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
];

const MONITOR_WORKSPACE_FLAG: &[ParameterDefinition] = &[
    ParameterDefinition {
        id: ParameterId::MONITOR,
        domain: ParameterDomain::Index,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
    ParameterDefinition {
        id: ParameterId::INDEX,
        domain: ParameterDomain::Index,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
    ParameterDefinition {
        id: ParameterId::ENABLED,
        domain: ParameterDomain::Flag,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
];

const MONITOR_WORKSPACE_LAYOUT: &[ParameterDefinition] = &[
    ParameterDefinition {
        id: ParameterId::MONITOR,
        domain: ParameterDomain::Index,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
    ParameterDefinition {
        id: ParameterId::INDEX,
        domain: ParameterDomain::Index,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
    ParameterDefinition {
        id: ParameterId::LAYOUT,
        domain: ParameterDomain::Layout,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
];

const MONITOR_WORKSPACE_CONTAINER: &[ParameterDefinition] = &[
    ParameterDefinition {
        id: ParameterId::MONITOR,
        domain: ParameterDomain::Index,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
    ParameterDefinition {
        id: ParameterId::INDEX,
        domain: ParameterDomain::Index,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
    ParameterDefinition {
        id: ParameterId::CONTAINER,
        domain: ParameterDomain::Index,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
];

const NAME: &[ParameterDefinition] = &[ParameterDefinition {
    id: ParameterId::NAME,
    domain: ParameterDomain::Name,
    cardinality: ArgumentCardinality::RequiredScalar,
}];

const BEHAVIOUR: &[ParameterDefinition] = &[ParameterDefinition {
    id: ParameterId::BEHAVIOUR,
    domain: ParameterDomain::Behaviour,
    cardinality: ArgumentCardinality::RequiredScalar,
}];

const IMPLEMENTATION: &[ParameterDefinition] = &[ParameterDefinition {
    id: ParameterId::IMPLEMENTATION,
    domain: ParameterDomain::Implementation,
    cardinality: ArgumentCardinality::RequiredScalar,
}];

const IMPLEMENTATION_FLAG: &[ParameterDefinition] = &[
    ParameterDefinition {
        id: ParameterId::IMPLEMENTATION,
        domain: ParameterDomain::Implementation,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
    ParameterDefinition {
        id: ParameterId::ENABLED,
        domain: ParameterDomain::Flag,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
];

const PATH: &[ParameterDefinition] = &[ParameterDefinition {
    id: ParameterId::PATH,
    domain: ParameterDomain::Path,
    cardinality: ArgumentCardinality::RequiredScalar,
}];

const EXE: &[ParameterDefinition] = &[ParameterDefinition {
    id: ParameterId::EXE,
    domain: ParameterDomain::Exe,
    cardinality: ArgumentCardinality::RequiredScalar,
}];

const TITLE_BAR: &[ParameterDefinition] = &[
    ParameterDefinition {
        id: ParameterId::IDENTIFIER,
        domain: ParameterDomain::Identifier,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
    ParameterDefinition {
        id: ParameterId::EXE,
        domain: ParameterDomain::Exe,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
];

const NAME_SIZE: &[ParameterDefinition] = &[
    ParameterDefinition {
        id: ParameterId::NAME,
        domain: ParameterDomain::Name,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
    ParameterDefinition {
        id: ParameterId::SIZE,
        domain: ParameterDomain::Size,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
];

const NAME_FLAG: &[ParameterDefinition] = &[
    ParameterDefinition {
        id: ParameterId::NAME,
        domain: ParameterDomain::Name,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
    ParameterDefinition {
        id: ParameterId::ENABLED,
        domain: ParameterDomain::Flag,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
];

const NAME_LAYOUT: &[ParameterDefinition] = &[
    ParameterDefinition {
        id: ParameterId::NAME,
        domain: ParameterDomain::Name,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
    ParameterDefinition {
        id: ParameterId::LAYOUT,
        domain: ParameterDomain::Layout,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
];

const NAME_PATH: &[ParameterDefinition] = &[
    ParameterDefinition {
        id: ParameterId::NAME,
        domain: ParameterDomain::Name,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
    ParameterDefinition {
        id: ParameterId::PATH,
        domain: ParameterDomain::Path,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
];

const LAYOUT_RULE: &[ParameterDefinition] = &[
    ParameterDefinition {
        id: ParameterId::MONITOR,
        domain: ParameterDomain::Index,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
    ParameterDefinition {
        id: ParameterId::INDEX,
        domain: ParameterDomain::Index,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
    ParameterDefinition {
        id: ParameterId::AT_COUNT,
        domain: ParameterDomain::AtCount,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
    ParameterDefinition {
        id: ParameterId::LAYOUT,
        domain: ParameterDomain::Layout,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
];

const CUSTOM_LAYOUT_RULE: &[ParameterDefinition] = &[
    ParameterDefinition {
        id: ParameterId::MONITOR,
        domain: ParameterDomain::Index,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
    ParameterDefinition {
        id: ParameterId::INDEX,
        domain: ParameterDomain::Index,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
    ParameterDefinition {
        id: ParameterId::AT_COUNT,
        domain: ParameterDomain::AtCount,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
    ParameterDefinition {
        id: ParameterId::PATH,
        domain: ParameterDomain::Path,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
];

const NAME_LAYOUT_RULE: &[ParameterDefinition] = &[
    ParameterDefinition {
        id: ParameterId::NAME,
        domain: ParameterDomain::Name,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
    ParameterDefinition {
        id: ParameterId::AT_COUNT,
        domain: ParameterDomain::AtCount,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
    ParameterDefinition {
        id: ParameterId::LAYOUT,
        domain: ParameterDomain::Layout,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
];

const NAME_CUSTOM_LAYOUT_RULE: &[ParameterDefinition] = &[
    ParameterDefinition {
        id: ParameterId::NAME,
        domain: ParameterDomain::Name,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
    ParameterDefinition {
        id: ParameterId::AT_COUNT,
        domain: ParameterDomain::AtCount,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
    ParameterDefinition {
        id: ParameterId::PATH,
        domain: ParameterDomain::Path,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
];

const MONITOR_NAMES: &[ParameterDefinition] = &[
    ParameterDefinition {
        id: ParameterId::MONITOR,
        domain: ParameterDomain::Index,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
    ParameterDefinition {
        id: ParameterId::NAMES,
        domain: ParameterDomain::Name,
        cardinality: ArgumentCardinality::RequiredList,
    },
];

const MONITOR_WORKSPACE_NAME: &[ParameterDefinition] = &[
    ParameterDefinition {
        id: ParameterId::MONITOR,
        domain: ParameterDomain::Index,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
    ParameterDefinition {
        id: ParameterId::INDEX,
        domain: ParameterDomain::Index,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
    ParameterDefinition {
        id: ParameterId::NAME,
        domain: ParameterDomain::Name,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
];

const MONITOR_WORKSPACE_PATH: &[ParameterDefinition] = &[
    ParameterDefinition {
        id: ParameterId::MONITOR,
        domain: ParameterDomain::Index,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
    ParameterDefinition {
        id: ParameterId::INDEX,
        domain: ParameterDomain::Index,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
    ParameterDefinition {
        id: ParameterId::PATH,
        domain: ParameterDomain::Path,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
];

const RATIOS: &[ParameterDefinition] = &[
    ParameterDefinition {
        id: ParameterId::COLUMN_RATIOS,
        domain: ParameterDomain::Ratios,
        cardinality: ArgumentCardinality::OptionalList,
    },
    ParameterDefinition {
        id: ParameterId::ROW_RATIOS,
        domain: ParameterDomain::Ratios,
        cardinality: ArgumentCardinality::OptionalList,
    },
];

const RESIZE_EDGE: &[ParameterDefinition] = &[
    ParameterDefinition {
        id: ParameterId::DIRECTION,
        domain: ParameterDomain::Direction,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
    ParameterDefinition {
        id: ParameterId::DELTA,
        domain: ParameterDomain::Pixels,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
];

const RESIZE_EDGE_BY_STEP: &[ParameterDefinition] = &[
    ParameterDefinition {
        id: ParameterId::DIRECTION,
        domain: ParameterDomain::Direction,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
    ParameterDefinition {
        id: ParameterId::SIZING,
        domain: ParameterDomain::Sizing,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
];

const RESIZE_STEP: &[ParameterDefinition] = &[ParameterDefinition {
    id: ParameterId::RESIZE_STEP,
    domain: ParameterDomain::ResizeStep,
    cardinality: ArgumentCardinality::RequiredScalar,
}];

const ALPHA: &[ParameterDefinition] = &[ParameterDefinition {
    id: ParameterId::ALPHA,
    domain: ParameterDomain::Alpha,
    cardinality: ArgumentCardinality::RequiredScalar,
}];

const BORDER_COLOUR: &[ParameterDefinition] = &[
    ParameterDefinition {
        id: ParameterId::WINDOW_KIND,
        domain: ParameterDomain::WindowKind,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
    ParameterDefinition {
        id: ParameterId::RED,
        domain: ParameterDomain::ColourChannel,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
    ParameterDefinition {
        id: ParameterId::GREEN,
        domain: ParameterDomain::ColourChannel,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
    ParameterDefinition {
        id: ParameterId::BLUE,
        domain: ParameterDomain::ColourChannel,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
];

const BORDER_WIDTH: &[ParameterDefinition] = &[ParameterDefinition {
    id: ParameterId::WIDTH,
    domain: ParameterDomain::BorderWidth,
    cardinality: ArgumentCardinality::RequiredScalar,
}];

const BORDER_OFFSET: &[ParameterDefinition] = &[ParameterDefinition {
    id: ParameterId::OFFSET,
    domain: ParameterDomain::BorderOffset,
    cardinality: ArgumentCardinality::RequiredScalar,
}];

const BORDER_STYLE: &[ParameterDefinition] = &[ParameterDefinition {
    id: ParameterId::STYLE,
    domain: ParameterDomain::BorderStyle,
    cardinality: ArgumentCardinality::RequiredScalar,
}];

const BORDER_IMPLEMENTATION: &[ParameterDefinition] = &[ParameterDefinition {
    id: ParameterId::IMPLEMENTATION,
    domain: ParameterDomain::BorderImplementation,
    cardinality: ArgumentCardinality::RequiredScalar,
}];

const STACKBAR_MODE: &[ParameterDefinition] = &[ParameterDefinition {
    id: ParameterId::MODE,
    domain: ParameterDomain::StackbarMode,
    cardinality: ArgumentCardinality::RequiredScalar,
}];

const STACKBAR_LABEL: &[ParameterDefinition] = &[ParameterDefinition {
    id: ParameterId::LABEL,
    domain: ParameterDomain::StackbarLabel,
    cardinality: ArgumentCardinality::RequiredScalar,
}];

const COLOUR: &[ParameterDefinition] = &[
    ParameterDefinition {
        id: ParameterId::RED,
        domain: ParameterDomain::ColourChannel,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
    ParameterDefinition {
        id: ParameterId::GREEN,
        domain: ParameterDomain::ColourChannel,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
    ParameterDefinition {
        id: ParameterId::BLUE,
        domain: ParameterDomain::ColourChannel,
        cardinality: ArgumentCardinality::RequiredScalar,
    },
];

const STACKBAR_HEIGHT: &[ParameterDefinition] = &[ParameterDefinition {
    id: ParameterId::HEIGHT,
    domain: ParameterDomain::StackbarHeight,
    cardinality: ArgumentCardinality::RequiredScalar,
}];
const STACKBAR_TAB_WIDTH: &[ParameterDefinition] = &[ParameterDefinition {
    id: ParameterId::TAB_WIDTH,
    domain: ParameterDomain::StackbarTabWidth,
    cardinality: ArgumentCardinality::RequiredScalar,
}];
const STACKBAR_FONT_SIZE: &[ParameterDefinition] = &[ParameterDefinition {
    id: ParameterId::FONT_SIZE,
    domain: ParameterDomain::StackbarFontSize,
    cardinality: ArgumentCardinality::RequiredScalar,
}];
const STACKBAR_FONT_FAMILY: &[ParameterDefinition] = &[ParameterDefinition {
    id: ParameterId::FONT_FAMILY,
    domain: ParameterDomain::StackbarFontFamily,
    cardinality: ArgumentCardinality::OptionalScalar,
}];

const fn def(
    id: ActionId,
    kind: BuiltinActionKind,
    category: ActionCategory,
    title: &'static str,
    description: &'static str,
    keywords: &'static [&'static str],
    parameters: &'static [ParameterDefinition],
) -> ActionDefinition {
    ActionDefinition {
        id,
        schema_version: ActionSchemaVersion::V1,
        kind,
        category,
        title,
        description,
        keywords,
        parameters,
        permitted_uses: BOTH_USES,
        confirmation: ConfirmationPolicy::None,
        undo: UndoPolicy::None,
    }
}

mod configuration;
mod window;
mod workspace;

pub use configuration::*;
pub use window::*;
pub use workspace::*;

impl BuiltinActionKind {
    #[must_use]
    pub const fn definition(self) -> &'static ActionDefinition {
        match self {
            Self::FocusWindow => &FOCUS_WINDOW,
            Self::MoveWindow => &MOVE_WINDOW,
            Self::ResizeWindow => &RESIZE_WINDOW,
            Self::ResizeWindowByStep => &RESIZE_WINDOW_BY_STEP,
            Self::SetWorkspaceLayout => &SET_WORKSPACE_LAYOUT,
            Self::ToggleWindowFloat => &TOGGLE_WINDOW_FLOAT,
            Self::CycleFocusWindow => &CYCLE_FOCUS_WINDOW,
            Self::CycleMoveWindow => &CYCLE_MOVE_WINDOW,
            Self::ToggleWindowMonocle => &TOGGLE_WINDOW_MONOCLE,
            Self::ToggleWindowMaximize => &TOGGLE_WINDOW_MAXIMIZE,
            Self::ToggleContainerLock => &TOGGLE_CONTAINER_LOCK,
            Self::StackWindow => &STACK_WINDOW,
            Self::UnstackWindow => &UNSTACK_WINDOW,
            Self::StackAll => &STACK_ALL,
            Self::UnstackAll => &UNSTACK_ALL,
            Self::CycleStack => &CYCLE_STACK,
            Self::CycleStackIndex => &CYCLE_STACK_INDEX,
            Self::FocusStackWindow => &FOCUS_STACK_WINDOW,
            Self::FocusWorkspace => &FOCUS_WORKSPACE,
            Self::CycleFocusWorkspace => &CYCLE_FOCUS_WORKSPACE,
            Self::CycleFocusEmptyWorkspace => &CYCLE_FOCUS_EMPTY_WORKSPACE,
            Self::FocusLastWorkspace => &FOCUS_LAST_WORKSPACE,
            Self::CloseWorkspace => &CLOSE_WORKSPACE,
            Self::FocusMonitor => &FOCUS_MONITOR,
            Self::CycleFocusMonitor => &CYCLE_FOCUS_MONITOR,
            Self::FocusMonitorAtCursor => &FOCUS_MONITOR_AT_CURSOR,
            Self::FocusWorkspaceOnAllMonitors => &FOCUS_WORKSPACE_ON_ALL_MONITORS,
            Self::FocusMonitorWorkspace => &FOCUS_MONITOR_WORKSPACE,
            Self::CloseWindow => &CLOSE_WINDOW,
            Self::MinimizeWindow => &MINIMIZE_WINDOW,
            Self::ForceFocus => &FORCE_FOCUS,
            Self::PromoteContainer => &PROMOTE_CONTAINER,
            Self::PromoteContainerSwap => &PROMOTE_CONTAINER_SWAP,
            Self::PromoteFocus => &PROMOTE_FOCUS,
            Self::PromoteWindow => &PROMOTE_WINDOW,
            Self::NewWorkspace => &NEW_WORKSPACE,
            Self::ToggleTiling => &TOGGLE_TILING,
            Self::CycleLayout => &CYCLE_LAYOUT,
            Self::FlipLayout => &FLIP_LAYOUT,
            Self::ToggleWorkspaceLayer => &TOGGLE_WORKSPACE_LAYER,
            Self::MoveContainerToLastWorkspace => &MOVE_CONTAINER_TO_LAST_WORKSPACE,
            Self::SendContainerToLastWorkspace => &SEND_CONTAINER_TO_LAST_WORKSPACE,
            Self::MoveContainerToWorkspace => &MOVE_CONTAINER_TO_WORKSPACE,
            Self::CycleMoveContainerToWorkspace => &CYCLE_MOVE_CONTAINER_TO_WORKSPACE,
            Self::SendContainerToWorkspace => &SEND_CONTAINER_TO_WORKSPACE,
            Self::CycleSendContainerToWorkspace => &CYCLE_SEND_CONTAINER_TO_WORKSPACE,
            Self::MoveContainerToMonitor => &MOVE_CONTAINER_TO_MONITOR,
            Self::CycleMoveContainerToMonitor => &CYCLE_MOVE_CONTAINER_TO_MONITOR,
            Self::SendContainerToMonitor => &SEND_CONTAINER_TO_MONITOR,
            Self::CycleSendContainerToMonitor => &CYCLE_SEND_CONTAINER_TO_MONITOR,
            Self::MoveContainerToMonitorWorkspace => &MOVE_CONTAINER_TO_MONITOR_WORKSPACE,
            Self::SendContainerToMonitorWorkspace => &SEND_CONTAINER_TO_MONITOR_WORKSPACE,
            Self::MoveWorkspaceToMonitor => &MOVE_WORKSPACE_TO_MONITOR,
            Self::CycleMoveWorkspaceToMonitor => &CYCLE_MOVE_WORKSPACE_TO_MONITOR,
            Self::SwapWorkspacesToMonitor => &SWAP_WORKSPACES_TO_MONITOR,
            Self::PreselectDirection => &PRESELECT_DIRECTION,
            Self::CancelPreselect => &CANCEL_PRESELECT,
            Self::Retile => &RETILE,
            Self::RetileWithResizeDimensions => &RETILE_WITH_RESIZE_DIMENSIONS,
            Self::ManageFocusedWindow => &MANAGE_FOCUSED_WINDOW,
            Self::UnmanageFocusedWindow => &UNMANAGE_FOCUSED_WINDOW,
            Self::AdjustContainerPadding => &ADJUST_CONTAINER_PADDING,
            Self::AdjustWorkspacePadding => &ADJUST_WORKSPACE_PADDING,
            Self::ToggleMouseFollowsFocus => &TOGGLE_MOUSE_FOLLOWS_FOCUS,
            Self::SetMouseFollowsFocus => &SET_MOUSE_FOLLOWS_FOCUS,
            Self::ToggleWindowContainerBehaviour => &TOGGLE_WINDOW_CONTAINER_BEHAVIOUR,
            Self::ToggleFloatOverride => &TOGGLE_FLOAT_OVERRIDE,
            Self::ToggleWorkspaceWindowContainerBehaviour => {
                &TOGGLE_WORKSPACE_WINDOW_CONTAINER_BEHAVIOUR
            }
            Self::ToggleWorkspaceFloatOverride => &TOGGLE_WORKSPACE_FLOAT_OVERRIDE,
            Self::ToggleCrossMonitorMoveBehaviour => &TOGGLE_CROSS_MONITOR_MOVE_BEHAVIOUR,
            Self::ToggleMonocleFocusBehaviour => &TOGGLE_MONOCLE_FOCUS_BEHAVIOUR,
            Self::TogglePause => &TOGGLE_PAUSE,
            Self::SetFocusedContainerPadding => &SET_FOCUSED_CONTAINER_PADDING,
            Self::SetFocusedWorkspacePadding => &SET_FOCUSED_WORKSPACE_PADDING,
            Self::SetContainerPadding => &SET_CONTAINER_PADDING,
            Self::SetWorkspacePadding => &SET_WORKSPACE_PADDING,
            Self::SetWorkspaceTiling => &SET_WORKSPACE_TILING,
            Self::SetWorkspaceMonocle => &SET_WORKSPACE_MONOCLE,
            Self::SetWorkspaceActiveContainerLock => &SET_WORKSPACE_ACTIVE_CONTAINER_LOCK,
            Self::SetMonitorWorkspaceLayout => &SET_MONITOR_WORKSPACE_LAYOUT,
            Self::EnsureWorkspaces => &ENSURE_WORKSPACES,
            Self::ClearWorkspaceLayoutRules => &CLEAR_WORKSPACE_LAYOUT_RULES,
            Self::SetScrollingColumns => &SET_SCROLLING_COLUMNS,
            Self::LockContainer => &LOCK_CONTAINER,
            Self::UnlockContainer => &UNLOCK_CONTAINER,
            Self::ToggleTitleBars => &TOGGLE_TITLE_BARS,
            Self::EnforceWorkspaceRules => &ENFORCE_WORKSPACE_RULES,
            Self::AddSessionFloatRule => &ADD_SESSION_FLOAT_RULE,
            Self::ClearSessionFloatRules => &CLEAR_SESSION_FLOAT_RULES,
            Self::ResizeWindowEdge => &RESIZE_WINDOW_EDGE,
            Self::ResizeWindowEdgeByStep => &RESIZE_WINDOW_EDGE_BY_STEP,
            Self::SetWindowHidingBehaviour => &SET_WINDOW_HIDING_BEHAVIOUR,
            Self::SetCrossMonitorMoveBehaviour => &SET_CROSS_MONITOR_MOVE_BEHAVIOUR,
            Self::SetMonocleFocusBehaviour => &SET_MONOCLE_FOCUS_BEHAVIOUR,
            Self::SetUnmanagedWindowOperationBehaviour => &SET_UNMANAGED_WINDOW_OPERATION_BEHAVIOUR,
            Self::SetFocusFollowsMouse => &SET_FOCUS_FOLLOWS_MOUSE,
            Self::ToggleFocusFollowsMouse => &TOGGLE_FOCUS_FOLLOWS_MOUSE,
            Self::AddWorkspaceLayoutRule => &ADD_WORKSPACE_LAYOUT_RULE,
            Self::FocusNamedWorkspace => &FOCUS_NAMED_WORKSPACE,
            Self::MoveContainerToNamedWorkspace => &MOVE_CONTAINER_TO_NAMED_WORKSPACE,
            Self::SendContainerToNamedWorkspace => &SEND_CONTAINER_TO_NAMED_WORKSPACE,
            Self::SetNamedWorkspaceContainerPadding => &SET_NAMED_WORKSPACE_CONTAINER_PADDING,
            Self::SetNamedWorkspacePadding => &SET_NAMED_WORKSPACE_PADDING,
            Self::SetNamedWorkspaceTiling => &SET_NAMED_WORKSPACE_TILING,
            Self::SetNamedWorkspaceLayout => &SET_NAMED_WORKSPACE_LAYOUT,
            Self::SetNamedWorkspaceCustomLayout => &SET_NAMED_WORKSPACE_CUSTOM_LAYOUT,
            Self::AddNamedWorkspaceLayoutRule => &ADD_NAMED_WORKSPACE_LAYOUT_RULE,
            Self::AddNamedWorkspaceCustomLayoutRule => &ADD_NAMED_WORKSPACE_CUSTOM_LAYOUT_RULE,
            Self::ClearNamedWorkspaceLayoutRules => &CLEAR_NAMED_WORKSPACE_LAYOUT_RULES,
            Self::EnsureNamedWorkspaces => &ENSURE_NAMED_WORKSPACES,
            Self::SetWorkspaceName => &SET_WORKSPACE_NAME,
            Self::SetLayoutRatios => &SET_LAYOUT_RATIOS,
            Self::SetCustomLayout => &SET_CUSTOM_LAYOUT,
            Self::SetWorkspaceCustomLayout => &SET_WORKSPACE_CUSTOM_LAYOUT,
            Self::AddWorkspaceCustomLayoutRule => &ADD_WORKSPACE_CUSTOM_LAYOUT_RULE,
            Self::EagerFocus => &EAGER_FOCUS,
            Self::RemoveTitleBar => &REMOVE_TITLE_BAR,
            Self::SetResizeStep => &SET_RESIZE_STEP,
            Self::SetTransparencyEnabled => &SET_TRANSPARENCY_ENABLED,
            Self::ToggleTransparency => &TOGGLE_TRANSPARENCY,
            Self::SetTransparencyAlpha => &SET_TRANSPARENCY_ALPHA,
            Self::SetBorderEnabled => &SET_BORDER_ENABLED,
            Self::SetBorderColour => &SET_BORDER_COLOUR,
            Self::SetBorderWidth => &SET_BORDER_WIDTH,
            Self::SetBorderOffset => &SET_BORDER_OFFSET,
            Self::SetBorderStyle => &SET_BORDER_STYLE,
            Self::SetBorderImplementation => &SET_BORDER_IMPLEMENTATION,
            Self::SetStackbarMode => &configuration::SET_STACKBAR_MODE,
            Self::SetStackbarLabel => &configuration::SET_STACKBAR_LABEL,
            Self::SetStackbarFocusedTextColour => &configuration::SET_STACKBAR_FOCUSED_TEXT_COLOUR,
            Self::SetStackbarUnfocusedTextColour => {
                &configuration::SET_STACKBAR_UNFOCUSED_TEXT_COLOUR
            }
            Self::SetStackbarBackgroundColour => &configuration::SET_STACKBAR_BACKGROUND_COLOUR,
            Self::SetStackbarHeight => &configuration::SET_STACKBAR_HEIGHT,
            Self::SetStackbarTabWidth => &configuration::SET_STACKBAR_TAB_WIDTH,
            Self::SetStackbarFontSize => &configuration::SET_STACKBAR_FONT_SIZE,
            Self::SetStackbarFontFamily => &configuration::SET_STACKBAR_FONT_FAMILY,
            Self::SetAnimationEnabled => &configuration::SET_ANIMATION_ENABLED,
            Self::SetAnimationDuration => &configuration::SET_ANIMATION_DURATION,
            Self::SetAnimationFps => &configuration::SET_ANIMATION_FPS,
            Self::SetAnimationStyle => &configuration::SET_ANIMATION_STYLE,
            Self::SetGlobalWorkAreaOffset => &configuration::SET_GLOBAL_WORK_AREA_OFFSET,
            Self::SetMonitorWorkAreaOffset => &configuration::SET_MONITOR_WORK_AREA_OFFSET,
            Self::SetWorkspaceWorkAreaOffset => &configuration::SET_WORKSPACE_WORK_AREA_OFFSET,
            Self::ToggleWindowBasedWorkAreaOffset => {
                &configuration::TOGGLE_WINDOW_BASED_WORK_AREA_OFFSET
            }
        }
    }
}

#[must_use]
pub fn definitions() -> [&'static ActionDefinition; 144] {
    BuiltinActionKind::ALL.map(BuiltinActionKind::definition)
}

#[must_use]
pub fn layout_name(layout: DefaultLayout) -> &'static str {
    match layout {
        DefaultLayout::BSP => "bsp",
        DefaultLayout::Columns => "columns",
        DefaultLayout::Rows => "rows",
        DefaultLayout::VerticalStack => "vertical-stack",
        DefaultLayout::HorizontalStack => "horizontal-stack",
        DefaultLayout::UltrawideVerticalStack => "ultrawide-vertical-stack",
        DefaultLayout::Grid => "grid",
        DefaultLayout::RightMainVerticalStack => "right-main-vertical-stack",
        DefaultLayout::Scrolling => "scrolling",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_has_one_definition_and_matching_id() {
        for kind in BuiltinActionKind::ALL {
            let definition = kind.definition();
            assert_eq!(definition.kind, kind);
            assert_eq!(definition.id, kind.id());
            assert_eq!(definition.schema_version, ActionSchemaVersion::V1);
            assert_eq!(
                definition.undo,
                UndoPolicy::None,
                "{kind:?} must not advertise undo until it captures an executable inverse"
            );
        }
    }

    #[test]
    fn collection_cardinality_is_part_of_the_parameter_contract() {
        let ensure_names = BuiltinActionKind::EnsureNamedWorkspaces.definition();
        assert!(ensure_names.parameters.iter().any(|parameter| {
            parameter.id == ParameterId::NAMES
                && parameter.cardinality() == ArgumentCardinality::RequiredList
        }));

        let ratios = BuiltinActionKind::SetLayoutRatios.definition();
        assert_eq!(ratios.parameters.len(), 2);
        assert!(ratios.parameters.iter().all(|parameter| {
            matches!(
                parameter.id,
                ParameterId::COLUMN_RATIOS | ParameterId::ROW_RATIOS
            ) && parameter.cardinality() == ArgumentCardinality::OptionalList
        }));
    }

    #[test]
    fn every_built_in_layout_has_a_stable_projection() {
        let names = [
            layout_name(DefaultLayout::BSP),
            layout_name(DefaultLayout::Columns),
            layout_name(DefaultLayout::Rows),
            layout_name(DefaultLayout::VerticalStack),
            layout_name(DefaultLayout::HorizontalStack),
            layout_name(DefaultLayout::UltrawideVerticalStack),
            layout_name(DefaultLayout::Grid),
            layout_name(DefaultLayout::RightMainVerticalStack),
            layout_name(DefaultLayout::Scrolling),
        ];
        assert_eq!(names.len(), 9);
        let mut unique = names.to_vec();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), 9);
    }
}
