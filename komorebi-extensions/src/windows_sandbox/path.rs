use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use super::LpacLaunchError;

pub(super) struct WideString(Vec<u16>);

impl WideString {
    pub(super) fn new(value: &str) -> Self {
        Self(value.encode_utf16().chain([0]).collect())
    }

    pub(super) const fn as_ptr(&self) -> *const u16 {
        self.0.as_ptr()
    }

    pub(super) const fn as_mut_ptr(&mut self) -> *mut u16 {
        self.0.as_mut_ptr()
    }
}

pub(super) struct WidePath(Vec<u16>);

impl WidePath {
    pub(super) fn new(path: &Path) -> Result<Self, LpacLaunchError> {
        if !path.is_absolute() {
            return Err(LpacLaunchError::InvalidWorkerPath);
        }
        let units: Vec<u16> = OsStr::new(path).encode_wide().collect();
        if units.contains(&0) {
            return Err(LpacLaunchError::InvalidWorkerPath);
        }
        Ok(Self(units.into_iter().chain([0]).collect()))
    }

    pub(super) const fn as_ptr(&self) -> *const u16 {
        self.0.as_ptr()
    }
}
