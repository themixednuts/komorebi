use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::Path;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum NativePathError {
    #[error("native path contains an interior NUL")]
    InteriorNul,
}

pub fn to_wide_null(path: &Path) -> Result<Vec<u16>, NativePathError> {
    let mut units = path.as_os_str().encode_wide().collect::<Vec<_>>();
    if units.contains(&0) {
        return Err(NativePathError::InteriorNul);
    }
    units.push(0);
    Ok(units)
}

pub fn os_string_from_wide(units: &[u16]) -> OsString {
    OsString::from_wide(units)
}

pub fn os_str_to_wide(value: &OsStr) -> Vec<u16> {
    value.encode_wide().collect()
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use std::path::{Path, Prefix};

    use super::{os_str_to_wide, os_string_from_wide, to_wide_null};

    #[test]
    fn unpaired_surrogate_round_trips_without_unicode_repair() {
        let units = [b'a'.into(), 0xD800, b'z'.into()];
        let value = os_string_from_wide(&units);

        assert_eq!(os_str_to_wide(&value), units);
    }

    #[test]
    fn wide_api_argument_preserves_verbatim_unc_prefix() -> Result<(), super::NativePathError> {
        let path = Path::new(r"\\?\UNC\server\share\name. ");
        let units = to_wide_null(path)?;

        assert_eq!(&units[..units.len() - 1], os_str_to_wide(path.as_os_str()));
        assert!(matches!(
            path.components().next(),
            Some(std::path::Component::Prefix(prefix))
                if matches!(prefix.kind(), Prefix::VerbatimUNC(_, _))
        ));
        Ok(())
    }

    #[test]
    fn standard_path_parser_distinguishes_device_namespace() {
        let value = OsString::from_wide(&r"\\.\PIPE\komorebi".encode_utf16().collect::<Vec<_>>());
        let path = Path::new(&value);

        assert!(matches!(
            path.components().next(),
            Some(std::path::Component::Prefix(prefix))
                if matches!(prefix.kind(), Prefix::DeviceNS(_))
        ));
    }
}
