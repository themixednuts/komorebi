use hex_color::HexColor;
#[cfg(feature = "schemars")]
use schemars::Schema;
#[cfg(feature = "schemars")]
use schemars::SchemaGenerator;

use crate::Color32;
use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(untagged)]
/// Colour representation
pub enum Colour {
    /// Colour represented as RGB
    Rgb(Rgb),
    /// Colour represented as Hex
    Hex(Hex),
}

impl From<Rgb> for Colour {
    fn from(value: Rgb) -> Self {
        Self::Rgb(value)
    }
}

impl From<u32> for Colour {
    fn from(value: u32) -> Self {
        Self::Rgb(Rgb::from(value))
    }
}

impl From<Color32> for Colour {
    fn from(value: Color32) -> Self {
        Colour::Rgb(Rgb::new(value.r(), value.g(), value.b()))
    }
}

impl From<Colour> for Color32 {
    fn from(value: Colour) -> Self {
        match value {
            Colour::Rgb(rgb) => Color32::from_rgb(rgb.r, rgb.g, rgb.b),
            Colour::Hex(hex) => {
                let rgb = Rgb::from(hex);
                Color32::from_rgb(rgb.r, rgb.g, rgb.b)
            }
        }
    }
}

/// Colour represented as a Hex string
#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq)]
pub struct Hex(pub HexColor);

#[cfg(feature = "schemars")]
impl schemars::JsonSchema for Hex {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("Hex")
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        schemars::json_schema!({
            "type": "string",
            "format": "color-hex",
            "description": "Colour represented as a Hex string"
        })
    }
}

impl From<Colour> for u32 {
    fn from(value: Colour) -> Self {
        match value {
            Colour::Rgb(val) => val.into(),
            Colour::Hex(val) => (Rgb::from(val)).into(),
        }
    }
}

#[derive(Debug, Copy, Clone, Eq, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Colour represented as RGB
pub struct Rgb {
    /// Red
    pub r: u8,
    /// Green
    pub g: u8,
    /// Blue
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }
}

impl From<Hex> for Rgb {
    fn from(value: Hex) -> Self {
        value.0.into()
    }
}

impl From<HexColor> for Rgb {
    fn from(value: HexColor) -> Self {
        Self {
            r: value.r,
            g: value.g,
            b: value.b,
        }
    }
}

impl From<Rgb> for u32 {
    fn from(value: Rgb) -> Self {
        u32::from_le_bytes([value.r, value.g, value.b, 0])
    }
}

impl From<u32> for Rgb {
    fn from(value: u32) -> Self {
        let [r, g, b, _] = value.to_le_bytes();
        Self { r, g, b }
    }
}

#[cfg(test)]
mod tests {
    use super::Rgb;

    #[test]
    fn rgb_channels_pack_and_unpack_without_overlap() {
        let rgb = Rgb::new(0x12, 0x34, 0x56);
        let packed = u32::from(rgb);

        assert_eq!(packed, 0x0056_3412);
        assert_eq!(Rgb::from(packed), rgb);
    }
}
