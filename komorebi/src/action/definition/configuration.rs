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

pub const SET_BORDER_ENABLED: ActionDefinition = ActionDefinition {
    id: ActionId::SET_BORDER_ENABLED,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::SetBorderEnabled,
    category: ActionCategory::Configuration,
    title: "Set borders enabled",
    description: "Enable or disable window borders",
    keywords: &["border", "enabled", "configuration"],
    parameters: FLAG,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const SET_BORDER_COLOUR: ActionDefinition = ActionDefinition {
    id: ActionId::SET_BORDER_COLOUR,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::SetBorderColour,
    category: ActionCategory::Configuration,
    title: "Set border colour",
    description: "Set the border colour for one window kind",
    keywords: &["border", "colour", "configuration"],
    parameters: BORDER_COLOUR,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const SET_BORDER_WIDTH: ActionDefinition = ActionDefinition {
    id: ActionId::SET_BORDER_WIDTH,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::SetBorderWidth,
    category: ActionCategory::Configuration,
    title: "Set border width",
    description: "Set the signed width used by komorebi borders",
    keywords: &["border", "width", "configuration"],
    parameters: BORDER_WIDTH,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const SET_BORDER_OFFSET: ActionDefinition = ActionDefinition {
    id: ActionId::SET_BORDER_OFFSET,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::SetBorderOffset,
    category: ActionCategory::Configuration,
    title: "Set border offset",
    description: "Set the signed offset used by komorebi borders",
    keywords: &["border", "offset", "configuration"],
    parameters: BORDER_OFFSET,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const SET_BORDER_STYLE: ActionDefinition = ActionDefinition {
    id: ActionId::SET_BORDER_STYLE,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::SetBorderStyle,
    category: ActionCategory::Configuration,
    title: "Set border style",
    description: "Set the corner style used by komorebi borders",
    keywords: &["border", "style", "configuration"],
    parameters: BORDER_STYLE,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

pub const SET_BORDER_IMPLEMENTATION: ActionDefinition = ActionDefinition {
    id: ActionId::SET_BORDER_IMPLEMENTATION,
    schema_version: ActionSchemaVersion::V1,
    kind: BuiltinActionKind::SetBorderImplementation,
    category: ActionCategory::Configuration,
    title: "Set border implementation",
    description: "Select komorebi borders or native Windows accent borders",
    keywords: &["border", "implementation", "configuration"],
    parameters: BORDER_IMPLEMENTATION,
    permitted_uses: BOTH_USES,
    confirmation: ConfirmationPolicy::None,
    undo: UndoPolicy::None,
};

macro_rules! stackbar_definition {
    ($name:ident, $kind:ident, $title:literal, $description:literal, $parameters:ident) => {
        pub const $name: ActionDefinition = ActionDefinition {
            id: ActionId::$name,
            schema_version: ActionSchemaVersion::V1,
            kind: BuiltinActionKind::$kind,
            category: ActionCategory::Configuration,
            title: $title,
            description: $description,
            keywords: &["stackbar", "configuration"],
            parameters: $parameters,
            permitted_uses: BOTH_USES,
            confirmation: ConfirmationPolicy::None,
            undo: UndoPolicy::None,
        };
    };
}

stackbar_definition!(
    SET_STACKBAR_MODE,
    SetStackbarMode,
    "Set stackbar mode",
    "Choose when stackbars are displayed",
    STACKBAR_MODE
);
stackbar_definition!(
    SET_STACKBAR_LABEL,
    SetStackbarLabel,
    "Set stackbar label",
    "Choose the text displayed on stackbar tabs",
    STACKBAR_LABEL
);
stackbar_definition!(
    SET_STACKBAR_FOCUSED_TEXT_COLOUR,
    SetStackbarFocusedTextColour,
    "Set focused stackbar text colour",
    "Set the focused stackbar tab text colour",
    COLOUR
);
stackbar_definition!(
    SET_STACKBAR_UNFOCUSED_TEXT_COLOUR,
    SetStackbarUnfocusedTextColour,
    "Set unfocused stackbar text colour",
    "Set the unfocused stackbar tab text colour",
    COLOUR
);
stackbar_definition!(
    SET_STACKBAR_BACKGROUND_COLOUR,
    SetStackbarBackgroundColour,
    "Set stackbar background colour",
    "Set the stackbar tab background colour",
    COLOUR
);
stackbar_definition!(
    SET_STACKBAR_HEIGHT,
    SetStackbarHeight,
    "Set stackbar height",
    "Set the stackbar height in pixels",
    STACKBAR_HEIGHT
);
stackbar_definition!(
    SET_STACKBAR_TAB_WIDTH,
    SetStackbarTabWidth,
    "Set stackbar tab width",
    "Set the stackbar tab width in pixels",
    STACKBAR_TAB_WIDTH
);
stackbar_definition!(
    SET_STACKBAR_FONT_SIZE,
    SetStackbarFontSize,
    "Set stackbar font size",
    "Set the stackbar font size; zero selects the system default",
    STACKBAR_FONT_SIZE
);
stackbar_definition!(
    SET_STACKBAR_FONT_FAMILY,
    SetStackbarFontFamily,
    "Set stackbar font family",
    "Set the stackbar font family; omit it to select the system default",
    STACKBAR_FONT_FAMILY
);
