use crate::core::DefaultLayout;

use super::builtin::BuiltinActionKind;
use super::id::ActionId;
use super::id::ActionSchemaVersion;
use super::id::ParameterId;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActionCategory {
    Window,
    Workspace,
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
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParameterDefinition {
    pub id: ParameterId,
    pub domain: ParameterDomain,
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
}];

const RESIZE: &[ParameterDefinition] = &[
    ParameterDefinition {
        id: ParameterId::AXIS,
        domain: ParameterDomain::Axis,
    },
    ParameterDefinition {
        id: ParameterId::DELTA,
        domain: ParameterDomain::Pixels,
    },
];

const LAYOUT: &[ParameterDefinition] = &[
    ParameterDefinition {
        id: ParameterId::WORKSPACE,
        domain: ParameterDomain::WorkspaceSelector,
    },
    ParameterDefinition {
        id: ParameterId::LAYOUT,
        domain: ParameterDomain::Layout,
    },
];

const WINDOW: &[ParameterDefinition] = &[ParameterDefinition {
    id: ParameterId::WINDOW,
    domain: ParameterDomain::WindowSelector,
}];

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
    undo: UndoPolicy::PriorManagerIntent,
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
    undo: UndoPolicy::PriorManagerIntent,
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
    undo: UndoPolicy::PriorManagerIntent,
};

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
    undo: UndoPolicy::PriorManagerIntent,
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
    undo: UndoPolicy::PriorManagerIntent,
};

impl BuiltinActionKind {
    #[must_use]
    pub const fn definition(self) -> &'static ActionDefinition {
        match self {
            Self::FocusWindow => &FOCUS_WINDOW,
            Self::MoveWindow => &MOVE_WINDOW,
            Self::ResizeWindow => &RESIZE_WINDOW,
            Self::SetWorkspaceLayout => &SET_WORKSPACE_LAYOUT,
            Self::ToggleWindowFloat => &TOGGLE_WINDOW_FLOAT,
        }
    }
}

#[must_use]
pub fn definitions() -> [&'static ActionDefinition; 5] {
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
        }
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
