use super::*;

pub const SET_RESIZE_STEP: ActionDefinition = ActionDefinition {
    id: ActionId::SET_RESIZE_STEP,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::SetResizeStep,
    category: ActionCategory::Configuration,
    title: "Set resize step",
    description: "Set the positive pixel step used by configured-step resize actions",
    keywords: &["resize", "step", "configuration"],
    parameters: RESIZE_STEP,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const SET_TRANSPARENCY_ENABLED: ActionDefinition = ActionDefinition {
    id: ActionId::SET_TRANSPARENCY_ENABLED,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::SetTransparencyEnabled,
    category: ActionCategory::Configuration,
    title: "Set transparency enabled",
    description: "Enable or disable transparency for unfocused windows",
    keywords: &["transparency", "enabled", "configuration"],
    parameters: FLAG,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const TOGGLE_TRANSPARENCY: ActionDefinition = ActionDefinition {
    id: ActionId::TOGGLE_TRANSPARENCY,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::ToggleTransparency,
    category: ActionCategory::Configuration,
    title: "Toggle transparency",
    description: "Toggle transparency for unfocused windows",
    keywords: &["transparency", "toggle", "configuration"],
    parameters: NONE,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const SET_TRANSPARENCY_ALPHA: ActionDefinition = ActionDefinition {
    id: ActionId::SET_TRANSPARENCY_ALPHA,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::SetTransparencyAlpha,
    category: ActionCategory::Configuration,
    title: "Set transparency alpha",
    description: "Set the alpha value used for unfocused-window transparency",
    keywords: &["transparency", "alpha", "configuration"],
    parameters: ALPHA,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};
