use std::{
    ffi::OsString,
    fmt::Write,
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
};

const PREFIX: &str = "komorebi-file:";

pub(crate) fn is_encoded(value: &str) -> bool {
    value.starts_with(PREFIX)
}

pub(crate) fn encode(path: &Path) -> String {
    path.as_os_str()
        .encode_wide()
        .fold(String::from(PREFIX), |mut key, code_unit| {
            let _ = write!(key, "{code_unit:04x}");
            key
        })
}

pub(crate) fn decode(key: &str) -> Result<PathBuf, DecodeError> {
    let encoded = key.strip_prefix(PREFIX).ok_or(DecodeError::Prefix)?;
    if encoded.len() % 4 != 0 {
        return Err(DecodeError::Length);
    }
    let wide = encoded
        .as_bytes()
        .chunks_exact(4)
        .map(|digits| {
            std::str::from_utf8(digits)
                .map_err(|_| DecodeError::Hex)
                .and_then(|digits| u16::from_str_radix(digits, 16).map_err(|_| DecodeError::Hex))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(PathBuf::from(OsString::from_wide(&wide)))
}

pub(crate) fn display(path: &Path) -> String {
    char::decode_utf16(path.as_os_str().encode_wide()).fold(
        String::new(),
        |mut rendered, character| {
            match character {
                Ok(character) => rendered.push(character),
                Err(error) => {
                    let _ = write!(rendered, "\\u{{{:04x}}}", error.unpaired_surrogate());
                }
            }
            rendered
        },
    )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub(crate) enum DecodeError {
    #[error("module name is not a komorebi file key")]
    Prefix,
    #[error("module file key has an incomplete UTF-16 code unit")]
    Length,
    #[error("module file key contains non-hexadecimal data")]
    Hex,
}

#[cfg(test)]
mod tests {
    use super::{decode, encode};
    use std::{ffi::OsString, os::windows::ffi::OsStringExt, path::PathBuf};

    #[test]
    fn key_round_trip_preserves_unpaired_utf16_surrogates() {
        let path = PathBuf::from(OsString::from_wide(&[
            u16::from(b'C'),
            u16::from(b':'),
            u16::from(b'\\'),
            0xd800,
            u16::from(b'\\'),
            u16::from(b'a'),
            u16::from(b'.'),
            u16::from(b't'),
            u16::from(b's'),
        ]));

        assert_eq!(decode(&encode(&path)), Ok(path));
    }
}
