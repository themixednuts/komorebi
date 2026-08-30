use crate::DEFAULT_RESIZE_STEP;
use crate::core::DEFAULT_TRANSPARENCY_ENABLED;
use crate::core::ResizeStep;
use crate::core::TransparencyAlpha;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConfigurationSnapshot {
    pub resize_step: ResizeStep,
    pub transparency: TransparencyConfiguration,
}

impl Default for ConfigurationSnapshot {
    fn default() -> Self {
        Self {
            resize_step: DEFAULT_RESIZE_STEP,
            transparency: TransparencyConfiguration::default(),
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
