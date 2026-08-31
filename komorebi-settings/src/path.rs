use std::ffi::OsString;
use std::path::Path;
use std::path::PathBuf;

use thiserror::Error;

pub(crate) fn encode(path: &Path) -> Vec<u8> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt as _;

        path.as_os_str()
            .encode_wide()
            .flat_map(u16::to_le_bytes)
            .collect()
    }
    #[cfg(not(windows))]
    {
        use std::os::unix::ffi::OsStrExt as _;

        path.as_os_str().as_bytes().to_vec()
    }
}

pub(crate) fn decode(encoded: &[u8]) -> Result<PathBuf, PathEncodingError> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStringExt as _;

        if !encoded.len().is_multiple_of(2) {
            return Err(PathEncodingError::OddUtf16ByteLength(encoded.len()));
        }
        let units = encoded
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        Ok(PathBuf::from(OsString::from_wide(&units)))
    }
    #[cfg(not(windows))]
    {
        use std::os::unix::ffi::OsStringExt as _;

        Ok(PathBuf::from(OsString::from_vec(encoded.to_vec())))
    }
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub(crate) enum PathEncodingError {
    #[error("stored WTF-16 path contains an odd byte count: {0}")]
    OddUtf16ByteLength(usize),
}
