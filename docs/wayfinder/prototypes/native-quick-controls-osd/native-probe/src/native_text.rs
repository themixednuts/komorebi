use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct NativeText {
    pub utf16: Vec<u16>,
    pub display: String,
}

impl From<Vec<u16>> for NativeText {
    fn from(utf16: Vec<u16>) -> Self {
        Self {
            display: String::from_utf16_lossy(&utf16),
            utf16,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::NativeText;

    #[test]
    fn malformed_surrogate_is_preserved_in_operational_units() {
        let units = vec![b'a'.into(), 0xD800, b'z'.into()];
        let text = NativeText::from(units.clone());

        assert_eq!(text.utf16, units);
        assert_eq!(text.display, "a\u{FFFD}z");
    }
}
