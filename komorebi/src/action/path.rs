use std::path::Path;
use std::path::PathBuf;

use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde::Serializer;
use serde::de::Error as _;
use thiserror::Error;

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct WindowsPath(PathBuf);

impl WindowsPath {
    pub fn new(path: impl Into<PathBuf>) -> Result<Self, WindowsPathError> {
        let path = path.into();
        if contains_nul(&path) {
            return Err(WindowsPathError::InteriorNul);
        }
        Ok(Self(path))
    }

    /// Creates a path from native Windows UTF-16 code units without Unicode repair.
    ///
    /// # Errors
    ///
    /// Returns [`WindowsPathError`] when the current platform cannot represent
    /// Windows path units or the input contains an interior NUL.
    pub fn from_wtf16(units: impl Into<Vec<u16>>) -> Result<Self, WindowsPathError> {
        Self::new(decode_wtf16(units.into())?)
    }

    #[must_use]
    pub fn as_path(&self) -> &Path {
        &self.0
    }

    #[must_use]
    pub fn into_path_buf(self) -> PathBuf {
        self.0
    }
}

impl AsRef<Path> for WindowsPath {
    fn as_ref(&self) -> &Path {
        self.as_path()
    }
}

impl TryFrom<PathBuf> for WindowsPath {
    type Error = WindowsPathError;

    fn try_from(path: PathBuf) -> Result<Self, Self::Error> {
        Self::new(path)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Error)]
pub enum WindowsPathError {
    #[error("Windows path contains an interior NUL code unit")]
    InteriorNul,
    #[error("path encoding is not valid on this platform")]
    WrongPlatformEncoding,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "encoding", content = "units", rename_all = "kebab-case")]
enum EncodedPath {
    Utf8(String),
    Wtf16(Vec<u16>),
    Bytes(Vec<u8>),
}

impl Serialize for WindowsPath {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if let Some(path) = self.0.to_str() {
            return EncodedPath::Utf8(path.to_string()).serialize(serializer);
        }

        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;

            EncodedPath::Wtf16(self.0.as_os_str().encode_wide().collect()).serialize(serializer)
        }

        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;

            EncodedPath::Bytes(self.0.as_os_str().as_bytes().to_vec()).serialize(serializer)
        }
    }
}

impl<'de> Deserialize<'de> for WindowsPath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let encoded = EncodedPath::deserialize(deserializer)?;
        let path = match encoded {
            EncodedPath::Utf8(path) => PathBuf::from(path),
            EncodedPath::Wtf16(units) => decode_wtf16(units).map_err(D::Error::custom)?,
            EncodedPath::Bytes(bytes) => decode_bytes(bytes).map_err(D::Error::custom)?,
        };
        Self::new(path).map_err(D::Error::custom)
    }
}

#[cfg(windows)]
fn contains_nul(path: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;

    path.as_os_str().encode_wide().any(|unit| unit == 0)
}

#[cfg(unix)]
fn contains_nul(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;

    path.as_os_str().as_bytes().contains(&0)
}

#[cfg(windows)]
fn decode_wtf16(units: Vec<u16>) -> Result<PathBuf, WindowsPathError> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;

    Ok(PathBuf::from(OsString::from_wide(&units)))
}

#[cfg(not(windows))]
fn decode_wtf16(_units: Vec<u16>) -> Result<PathBuf, WindowsPathError> {
    Err(WindowsPathError::WrongPlatformEncoding)
}

#[cfg(unix)]
fn decode_bytes(bytes: Vec<u8>) -> Result<PathBuf, WindowsPathError> {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    Ok(PathBuf::from(OsString::from_vec(bytes)))
}

#[cfg(not(unix))]
fn decode_bytes(_bytes: Vec<u8>) -> Result<PathBuf, WindowsPathError> {
    Err(WindowsPathError::WrongPlatformEncoding)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unicode_path_round_trips() {
        let path = WindowsPath::new(r"C:\Users\jonfo\レイアウト.json").unwrap();
        let encoded = serde_json::to_string(&path).unwrap();
        let decoded: WindowsPath = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, path);
    }

    #[cfg(windows)]
    #[test]
    fn direct_wtf16_construction_preserves_unpaired_surrogates() {
        use std::os::windows::ffi::OsStrExt as _;

        let units = [b'C' as u16, b':' as u16, b'\\' as u16, 0xD800, b'x' as u16];
        let path = WindowsPath::from_wtf16(units).unwrap();
        assert_eq!(
            path.as_path().as_os_str().encode_wide().collect::<Vec<_>>(),
            units
        );
    }

    #[cfg(windows)]
    #[test]
    fn wtf16_path_round_trips_without_replacement() {
        use std::ffi::OsString;
        use std::os::windows::ffi::OsStrExt;
        use std::os::windows::ffi::OsStringExt;

        let units = [b'C' as u16, b':' as u16, b'\\' as u16, 0xD800, b'x' as u16];
        let path = WindowsPath::new(PathBuf::from(OsString::from_wide(&units))).unwrap();
        let encoded = serde_json::to_string(&path).unwrap();
        let decoded: WindowsPath = serde_json::from_str(&encoded).unwrap();
        let decoded_units: Vec<u16> = decoded.as_path().as_os_str().encode_wide().collect();
        assert_eq!(decoded_units, units);
    }

    #[cfg(windows)]
    #[test]
    fn interior_nul_is_rejected() {
        use std::ffi::OsString;
        use std::os::windows::ffi::OsStringExt;

        let path = PathBuf::from(OsString::from_wide(&[b'C' as u16, 0, b'x' as u16]));
        assert_eq!(WindowsPath::new(path), Err(WindowsPathError::InteriorNul));
    }
}
