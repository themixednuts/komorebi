use std::os::windows::ffi::OsStrExt;
use std::path::Path;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WidePath(Vec<u16>);

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum WidePathError {
    #[error("Windows path contains an interior NUL at UTF-16 unit {unit}")]
    InteriorNul { unit: usize },
}

impl WidePath {
    /// Encodes a native Windows path without requiring valid Unicode.
    ///
    /// # Errors
    ///
    /// Returns an error when the path contains an interior NUL, which a NUL-terminated Win32
    /// boundary cannot represent without truncation.
    pub fn new(path: &Path) -> Result<Self, WidePathError> {
        let mut units: Vec<u16> = path.as_os_str().encode_wide().collect();
        if let Some(unit) = units.iter().position(|value| *value == 0) {
            return Err(WidePathError::InteriorNul { unit });
        }
        units.push(0);
        Ok(Self(units))
    }

    #[must_use]
    pub fn units_with_nul(&self) -> &[u16] {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn preserves_windows_prefixes_and_trailing_spelling() -> Result<(), WidePathError> {
        for source in [
            r"\\server\share\folder\file.txt",
            r"\\?\C:\folder\trailing. ",
            r"\\?\UNC\server\share\folder\file.txt",
        ] {
            let path = Path::new(source);
            let encoded = WidePath::new(path)?;
            let expected: Vec<u16> = path
                .as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            assert_eq!(encoded.units_with_nul(), expected);
        }
        Ok(())
    }

    #[test]
    fn preserves_ill_formed_utf16() -> Result<(), WidePathError> {
        let source = [u16::from(b'C'), u16::from(b':'), u16::from(b'\\'), 0xd800];
        let path = PathBuf::from(OsString::from_wide(&source));
        let encoded = WidePath::new(&path)?;
        let mut expected = source.to_vec();
        expected.push(0);
        assert_eq!(encoded.units_with_nul(), expected);
        Ok(())
    }

    #[test]
    fn rejects_interior_nul() {
        let path = PathBuf::from(OsString::from_wide(&[u16::from(b'a'), 0, u16::from(b'b')]));
        assert_eq!(
            WidePath::new(&path),
            Err(WidePathError::InteriorNul { unit: 1 })
        );
    }
}
