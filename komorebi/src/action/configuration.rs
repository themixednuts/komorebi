use crate::DEFAULT_RESIZE_STEP;
use std::sync::Arc;

use crate::animation::DEFAULT_ANIMATION_DURATION;
use crate::animation::DEFAULT_ANIMATION_ENABLED;
use crate::animation::DEFAULT_ANIMATION_STYLE;
use crate::animation::default_animation_fps;
use crate::animation::prefix::AnimationPrefix;
use crate::core::AnimationDuration;
use crate::core::AnimationFps;
use crate::core::AnimationStyle;
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
    pub animation: Arc<AnimationConfiguration>,
}

impl Default for ConfigurationSnapshot {
    fn default() -> Self {
        Self {
            resize_step: DEFAULT_RESIZE_STEP,
            border: BorderConfiguration::default(),
            transparency: TransparencyConfiguration::default(),
            stackbar: StackbarConfiguration::default(),
            animation: Arc::new(AnimationConfiguration::default()),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AnimationConfiguration {
    pub enabled: ScopedAnimationValue<bool>,
    pub duration: ScopedAnimationValue<AnimationDuration>,
    pub style: ScopedAnimationValue<AnimationStyleSnapshot>,
    pub fps: AnimationFps,
}

impl Default for AnimationConfiguration {
    fn default() -> Self {
        Self {
            enabled: ScopedAnimationValue::global(DEFAULT_ANIMATION_ENABLED),
            duration: ScopedAnimationValue::global(AnimationDuration::new(
                DEFAULT_ANIMATION_DURATION,
            )),
            style: ScopedAnimationValue::global(DEFAULT_ANIMATION_STYLE.into()),
            fps: default_animation_fps(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScopedAnimationValue<T> {
    pub global: T,
    pub movement: Option<T>,
    pub transparency: Option<T>,
}

impl<T> ScopedAnimationValue<T> {
    pub const fn global(value: T) -> Self {
        Self {
            global: value,
            movement: None,
            transparency: None,
        }
    }

    pub fn set(&mut self, prefix: Option<AnimationPrefix>, value: T) {
        match prefix {
            Some(AnimationPrefix::Movement) => self.movement = Some(value),
            Some(AnimationPrefix::Transparency) => self.transparency = Some(value),
            None => {
                self.global = value;
                self.movement = None;
                self.transparency = None;
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AnimationStyleSnapshot {
    Named(Box<str>),
    CubicBezier([u64; 4]),
}

impl From<AnimationStyle> for AnimationStyleSnapshot {
    fn from(value: AnimationStyle) -> Self {
        match value {
            AnimationStyle::CubicBezier(x1, y1, x2, y2) => {
                Self::CubicBezier([x1.to_bits(), y1.to_bits(), x2.to_bits(), y2.to_bits()])
            }
            named => Self::Named(named.to_string().into_boxed_str()),
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
