use std::ptr;

use windows_core::HRESULT;
use windows_core::PCWSTR;
use windows_sys::Win32::Foundation::ERROR_ALREADY_EXISTS;
use windows_sys::Win32::Foundation::LocalFree;
use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows_sys::Win32::Security::FreeSid;
use windows_sys::Win32::Security::Isolation::CreateAppContainerProfile;
use windows_sys::Win32::Security::Isolation::DeriveAppContainerSidFromAppContainerName;
use windows_sys::Win32::Security::Isolation::GetAppContainerFolderPath;
use windows_sys::Win32::Security::PSID;
use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::System::SystemInformation::GetSystemWindowsDirectoryW;

use super::LpacLaunchError;
use super::path::WideString;

pub(super) struct AppContainerSid(PSID);

impl AppContainerSid {
    pub(super) fn open_or_create(identity: &WideString) -> Result<Self, LpacLaunchError> {
        let display_name = WideString::new("Komorebi extension worker");
        let description = WideString::new("Isolated Komorebi Lua extension worker");
        let mut sid = ptr::null_mut();
        let result = unsafe {
            // SAFETY: strings are NUL-terminated and the output pointer is writable.
            CreateAppContainerProfile(
                identity.as_ptr(),
                display_name.as_ptr(),
                description.as_ptr(),
                ptr::null(),
                0,
                &raw mut sid,
            )
        };
        if result == HRESULT::from_win32(ERROR_ALREADY_EXISTS).0 {
            let derive_result = unsafe {
                // SAFETY: identity is NUL-terminated and the output pointer is writable.
                DeriveAppContainerSidFromAppContainerName(identity.as_ptr(), &raw mut sid)
            };
            if derive_result < 0 {
                return Err(LpacLaunchError::Hresult {
                    operation: "DeriveAppContainerSidFromAppContainerName",
                    hresult: derive_result,
                });
            }
        } else if result < 0 {
            return Err(LpacLaunchError::Hresult {
                operation: "CreateAppContainerProfile",
                hresult: result,
            });
        }
        if sid.is_null() {
            return Err(LpacLaunchError::InvalidHandle);
        }
        Ok(Self(sid))
    }

    pub(super) const fn as_ptr(&self) -> PSID {
        self.0
    }
}

impl Drop for AppContainerSid {
    fn drop(&mut self) {
        unsafe {
            // SAFETY: allocated by an AppContainer SID API and freed once here.
            FreeSid(self.0);
        }
    }
}

pub(super) struct AppContainerEnvironment(Vec<u16>);

impl AppContainerEnvironment {
    pub(super) fn new(sid: &AppContainerSid) -> Result<Self, LpacLaunchError> {
        let sid_string = SidString::new(sid.as_ptr())?;
        let profile = AppContainerFolder::new(sid_string.as_ptr())?;
        let profile_units = profile.units();
        let system_root = system_windows_directory()?;
        let mut temp = profile_units.to_vec();
        temp.extend("\\Temp".encode_utf16());

        let mut block = Vec::with_capacity(256);
        push_environment_entry(&mut block, "LOCALAPPDATA", profile_units);
        push_environment_entry(&mut block, "SystemRoot", &system_root);
        push_environment_entry(&mut block, "TEMP", &temp);
        push_environment_entry(&mut block, "TMP", &temp);
        block.push(0);
        Ok(Self(block))
    }

    pub(super) const fn as_ptr(&self) -> *const u16 {
        self.0.as_ptr()
    }
}

struct SidString(*mut u16);

impl SidString {
    fn new(sid: PSID) -> Result<Self, LpacLaunchError> {
        let mut value = ptr::null_mut();
        if unsafe {
            // SAFETY: the SID is live and Windows allocates the returned NUL-terminated string.
            ConvertSidToStringSidW(sid, &raw mut value)
        } == 0
        {
            return Err(LpacLaunchError::windows("ConvertSidToStringSidW"));
        }
        if value.is_null() {
            return Err(LpacLaunchError::InvalidHandle);
        }
        Ok(Self(value))
    }

    const fn as_ptr(&self) -> *const u16 {
        self.0
    }
}

impl Drop for SidString {
    fn drop(&mut self) {
        unsafe {
            // SAFETY: allocated by ConvertSidToStringSidW and freed exactly once.
            LocalFree(self.0.cast());
        }
    }
}

struct AppContainerFolder(*mut u16);

impl AppContainerFolder {
    fn new(sid: *const u16) -> Result<Self, LpacLaunchError> {
        let mut value = ptr::null_mut();
        let result = unsafe {
            // SAFETY: SID text is NUL-terminated and output storage is writable.
            GetAppContainerFolderPath(sid, &raw mut value)
        };
        if result < 0 {
            return Err(LpacLaunchError::Hresult {
                operation: "GetAppContainerFolderPath",
                hresult: result,
            });
        }
        if value.is_null() {
            return Err(LpacLaunchError::InvalidHandle);
        }
        Ok(Self(value))
    }

    fn units(&self) -> &[u16] {
        let value = PCWSTR::from_raw(self.0);
        let length = unsafe {
            // SAFETY: GetAppContainerFolderPath guarantees a live NUL-terminated allocation.
            value.len()
        };
        unsafe {
            // SAFETY: the allocation remains owned by self and contains `length` UTF-16 units.
            std::slice::from_raw_parts(self.0, length)
        }
    }
}

impl Drop for AppContainerFolder {
    fn drop(&mut self) {
        unsafe {
            // SAFETY: allocated by GetAppContainerFolderPath and freed exactly once.
            CoTaskMemFree(self.0.cast());
        }
    }
}

fn system_windows_directory() -> Result<Vec<u16>, LpacLaunchError> {
    let mut directory = vec![0_u16; 32_768];
    let capacity =
        u32::try_from(directory.len()).map_err(|_| LpacLaunchError::StructureSizeOverflow)?;
    let length = unsafe {
        // SAFETY: the buffer is writable for the declared capacity.
        GetSystemWindowsDirectoryW(directory.as_mut_ptr(), capacity)
    };
    if length == 0 {
        return Err(LpacLaunchError::windows("GetSystemWindowsDirectoryW"));
    }
    let length = usize::try_from(length).map_err(|_| LpacLaunchError::StructureSizeOverflow)?;
    if length >= directory.len() {
        return Err(LpacLaunchError::StructureSizeOverflow);
    }
    directory.truncate(length);
    Ok(directory)
}

fn push_environment_entry(block: &mut Vec<u16>, key: &str, value: &[u16]) {
    block.extend(key.encode_utf16());
    block.push(u16::from(b'='));
    block.extend_from_slice(value);
    block.push(0);
}
