use crate::DEFAULT_RESIZE_STEP;
use crate::core::BorderImplementation;
use crate::core::BorderOffset;
use crate::core::BorderStyle;
use crate::core::BorderWidth;
use crate::core::DEFAULT_STACKBAR_BACKGROUND_COLOUR;
use crate::core::DEFAULT_STACKBAR_FOCUSED_TEXT_COLOUR;
use crate::core::DEFAULT_STACKBAR_LABEL;
use crate::core::DEFAULT_STACKBAR_MODE;
use crate::core::DEFAULT_STACKBAR_UNFOCUSED_TEXT_COLOUR;
use crate::core::DEFAULT_TRANSPARENCY_ENABLED;
use crate::core::ResizeStep;
use crate::core::StackbarFontSize;
use crate::core::StackbarHeight;
use crate::core::StackbarLabel;
use crate::core::StackbarMode;
use crate::core::StackbarTabWidth;
use crate::core::TransparencyAlpha;
use komorebi_themes::colour::Rgb;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfigurationSnapshot {
    pub resize_step: ResizeStep,
    pub border: BorderConfiguration,
    pub transparency: TransparencyConfiguration,
    pub stackbar: StackbarConfiguration,
}

impl Default for ConfigurationSnapshot {
    fn default() -> Self {
        Self {
            resize_step: DEFAULT_RESIZE_STEP,
            border: BorderConfiguration::default(),
            transparency: TransparencyConfiguration::default(),
            stackbar: StackbarConfiguration::default(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StackbarConfiguration {
    pub mode: StackbarMode,
    pub label: StackbarLabel,
    pub focused_text_colour: Rgb,
    pub unfocused_text_colour: Rgb,
    pub background_colour: Rgb,
    pub height: StackbarHeight,
    pub tab_width: StackbarTabWidth,
    pub font_size: StackbarFontSize,
    pub font_family: Option<Box<str>>,
}

impl Default for StackbarConfiguration {
    fn default() -> Self {
        Self {
            mode: DEFAULT_STACKBAR_MODE,
            label: DEFAULT_STACKBAR_LABEL,
            focused_text_colour: DEFAULT_STACKBAR_FOCUSED_TEXT_COLOUR.into(),
            unfocused_text_colour: DEFAULT_STACKBAR_UNFOCUSED_TEXT_COLOUR.into(),
            background_colour: DEFAULT_STACKBAR_BACKGROUND_COLOUR.into(),
            height: StackbarHeight::DEFAULT,
            tab_width: StackbarTabWidth::DEFAULT,
            font_size: StackbarFontSize::DEFAULT,
            font_family: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BorderConfiguration {
    pub enabled: bool,
    pub width: BorderWidth,
    pub offset: BorderOffset,
    pub style: BorderStyle,
    pub implementation: BorderImplementation,
}

impl Default for BorderConfiguration {
    fn default() -> Self {
        Self {
            enabled: true,
            width: BorderWidth::DEFAULT,
            offset: BorderOffset::DEFAULT,
            style: BorderStyle::System,
            implementation: BorderImplementation::Komorebi,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransparencyConfiguration {
    pub enabled: bool,
    pub alpha: TransparencyAlpha,
}

impl Default for TransparencyConfiguration {
    fn default() -> Self {
        Self {
            enabled: DEFAULT_TRANSPARENCY_ENABLED,
            alpha: TransparencyAlpha::DEFAULT,
        }
    }
}
