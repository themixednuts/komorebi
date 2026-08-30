use serde::Deserialize;
use serde::Serialize;

use super::StackbarLabel;
use super::StackbarMode;

pub const DEFAULT_STACKBAR_MODE: StackbarMode = StackbarMode::Never;
pub const DEFAULT_STACKBAR_LABEL: StackbarLabel = StackbarLabel::Title;
pub const DEFAULT_STACKBAR_FOCUSED_TEXT_COLOUR: u32 = 0x00ff_ffff;
pub const DEFAULT_STACKBAR_UNFOCUSED_TEXT_COLOUR: u32 = 0x00b3_b3b3;
pub const DEFAULT_STACKBAR_BACKGROUND_COLOUR: u32 = 0x0033_3333;

macro_rules! stackbar_pixels {
    ($name:ident, $default:expr) => {
        #[derive(
            Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
        )]
        #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
        #[serde(transparent)]
        pub struct $name(i32);

        impl $name {
            pub const DEFAULT: Self = Self($default);

            #[must_use]
            pub const fn new(value: i32) -> Self {
                Self(value)
            }

            #[must_use]
            pub const fn get(self) -> i32 {
                self.0
            }
        }
    };
}

stackbar_pixels!(StackbarHeight, 40);
stackbar_pixels!(StackbarTabWidth, 200);
stackbar_pixels!(StackbarFontSize, 0);
