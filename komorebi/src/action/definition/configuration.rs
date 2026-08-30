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
