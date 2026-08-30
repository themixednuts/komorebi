use crate::DEFAULT_RESIZE_STEP;
use crate::core::BorderImplementation;
use crate::core::BorderOffset;
use crate::core::BorderStyle;
use crate::core::BorderWidth;
use crate::core::DEFAULT_TRANSPARENCY_ENABLED;
use crate::core::ResizeStep;
use crate::core::TransparencyAlpha;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConfigurationSnapshot {
    pub resize_step: ResizeStep,
    pub border: BorderConfiguration,
    pub transparency: TransparencyConfiguration,
}

impl Default for ConfigurationSnapshot {
    fn default() -> Self {
        Self {
            resize_step: DEFAULT_RESIZE_STEP,
            border: BorderConfiguration::default(),
            transparency: TransparencyConfiguration::default(),
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
