use std::cmp::Ordering;
use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::OsStrExt;

use anyhow::{Result, ensure};
use windows_sys::Win32::Globalization::{
    CSTR_EQUAL, CSTR_GREATER_THAN, CSTR_LESS_THAN, CompareStringOrdinal,
};

pub(super) struct EnvironmentEntry {
    name: OsString,
    value: OsString,
}

impl EnvironmentEntry {
    pub(super) fn new(name: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
        }
    }
}

pub(super) struct EnvironmentBlock(Vec<u16>);

impl EnvironmentBlock {
    pub(super) fn build(overrides: Vec<EnvironmentEntry>, include_all: bool) -> Result<Self> {
        let mut entries: Vec<_> = if include_all {
            std::env::vars_os()
                .map(|(name, value)| EnvironmentEntry { name, value })
                .collect()
        } else {
            inherited_environment()
        };

        for entry in overrides {
            entries.retain(|candidate| !same_name(&candidate.name, &entry.name));
            entries.push(entry);
        }
        entries.sort_unstable_by(|left, right| compare_names(&left.name, &right.name));

        let mut block = Vec::new();
        for entry in entries {
            append_entry(&mut block, &entry)?;
        }
        block.push(0);
        Ok(Self(block))
    }

    pub(super) fn as_ptr(&self) -> *const u16 {
        self.0.as_ptr()
    }
}

fn inherited_environment() -> Vec<EnvironmentEntry> {
    const NAMES: &[&str] = &[
        "SYSTEMROOT",
        "WINDIR",
        "COMSPEC",
        "PATH",
        "PATHEXT",
        "USERNAME",
        "USERDOMAIN",
        "USERPROFILE",
        "HOMEDRIVE",
        "HOMEPATH",
        "LOCALAPPDATA",
        "APPDATA",
        "PROGRAMDATA",
        "PROGRAMFILES",
        "PROGRAMFILES(X86)",
        "COMMONPROGRAMFILES",
        "NUMBER_OF_PROCESSORS",
        "OS",
        "PROCESSOR_ARCHITECTURE",
        "TEMP",
        "TMP",
    ];
    NAMES
        .iter()
        .filter_map(|name| std::env::var_os(name).map(|value| EnvironmentEntry::new(*name, value)))
        .collect()
}

fn append_entry(block: &mut Vec<u16>, entry: &EnvironmentEntry) -> Result<()> {
    let name: Vec<_> = entry.name.encode_wide().collect();
    let value: Vec<_> = entry.value.encode_wide().collect();
    ensure!(!name.is_empty(), "environment variable name is empty");
    ensure!(
        !name.contains(&0) && !name.contains(&u16::from(b'=')),
        "environment variable name contains a forbidden code unit"
    );
    ensure!(
        !value.contains(&0),
        "environment variable value contains an interior NUL"
    );
    block.extend(name);
    block.push(u16::from(b'='));
    block.extend(value);
    block.push(0);
    Ok(())
}

fn same_name(left: &OsStr, right: &OsStr) -> bool {
    compare_names(left, right) == Ordering::Equal
}

fn compare_names(left: &OsStr, right: &OsStr) -> Ordering {
    let left: Vec<_> = left.encode_wide().collect();
    let right: Vec<_> = right.encode_wide().collect();
    let left_len = i32::try_from(left.len()).unwrap_or(i32::MAX);
    let right_len = i32::try_from(right.len()).unwrap_or(i32::MAX);
    // SAFETY: both pointers are valid for their explicit lengths. CompareStringOrdinal accepts
    // potentially ill-formed UTF-16 and performs the case-insensitive ordering Windows uses for
    // environment blocks.
    match unsafe { CompareStringOrdinal(left.as_ptr(), left_len, right.as_ptr(), right_len, 1) } {
        CSTR_LESS_THAN => Ordering::Less,
        CSTR_EQUAL => Ordering::Equal,
        CSTR_GREATER_THAN => Ordering::Greater,
        _ => left.cmp(&right),
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};

    use super::{EnvironmentBlock, EnvironmentEntry};

    #[test]
    fn preserves_ill_formed_utf16_values() {
        let value = OsString::from_wide(&[u16::from(b'a'), 0xD800, u16::from(b'z')]);
        let expected: Vec<_> = value.encode_wide().collect();
        let block =
            EnvironmentBlock::build(vec![EnvironmentEntry::new("WAYFINDER_WTF16", value)], false)
                .expect("environment block should accept WTF-16");
        assert!(
            block
                .0
                .windows(expected.len())
                .any(|window| window == expected)
        );
    }

    #[test]
    fn an_override_replaces_case_insensitive_duplicates() {
        let block = EnvironmentBlock::build(
            vec![
                EnvironmentEntry::new("Wayfinder_Key", "old"),
                EnvironmentEntry::new("WAYFINDER_KEY", "new"),
            ],
            false,
        )
        .expect("environment block should be valid");
        let text = String::from_utf16(&block.0).expect("ASCII fixture should decode");
        assert!(!text.contains("old"));
        assert!(text.contains("WAYFINDER_KEY=new"));
    }
}
