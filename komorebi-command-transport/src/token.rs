use std::ffi::c_void;
use std::io;
use std::mem::offset_of;
use std::mem::size_of;
use std::ptr;
use std::slice;

use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::Foundation::LocalFree;
use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows_sys::Win32::Security::CopySid;
use windows_sys::Win32::Security::GetLengthSid;
use windows_sys::Win32::Security::GetTokenInformation;
use windows_sys::Win32::Security::IsValidSid;
use windows_sys::Win32::Security::PSID;
use windows_sys::Win32::Security::SID_AND_ATTRIBUTES;
use windows_sys::Win32::Security::TOKEN_GROUPS;
use windows_sys::Win32::Security::TOKEN_QUERY;
use windows_sys::Win32::Security::TokenGroups;
use windows_sys::Win32::System::SystemServices::SE_GROUP_LOGON_ID;
use windows_sys::Win32::System::Threading::GetCurrentProcess;
use windows_sys::Win32::System::Threading::GetCurrentThread;
use windows_sys::Win32::System::Threading::OpenProcessToken;
use windows_sys::Win32::System::Threading::OpenThreadToken;

use crate::TransportError;

const MAX_SID_STRING_UNITS: usize = 256;

#[derive(Clone, Eq, Hash, PartialEq)]
pub struct LogonSid(Box<[u8]>);

impl std::fmt::Debug for LogonSid {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_tuple("LogonSid").field(&self.0).finish()
    }
}

impl LogonSid {
    /// Copies the current process token's logon SID into an owned value.
    ///
    /// # Errors
    ///
    /// Returns an error when token access fails or its group data is malformed.
    pub fn current() -> Result<Self, TransportError> {
        let token = OwnedToken::current_process()?;
        Self::from_token(&token)
    }

    pub(crate) fn current_thread() -> Result<Self, TransportError> {
        let token = OwnedToken::current_thread()?;
        Self::from_token(&token)
    }

    pub(crate) fn to_sddl(&self) -> Result<String, TransportError> {
        let mut raw = ptr::null_mut();
        // SAFETY: `self` owns a validated SID and `raw` is a writable output pointer.
        if unsafe { ConvertSidToStringSidW(self.as_psid(), &raw mut raw) } == 0 {
            return Err(TransportError::windows("ConvertSidToStringSidW"));
        }
        let string = LocalWideString(raw);
        let length = (0..=MAX_SID_STRING_UNITS)
            .find(|&index| {
                // SAFETY: Windows returned a NUL-terminated SID string; the bounded
                // scan prevents an unbounded read if that contract is violated.
                unsafe { *string.0.add(index) == 0 }
            })
            .ok_or(TransportError::MalformedToken)?;
        // SAFETY: the preceding scan proved that `length` initialized UTF-16 code
        // units precede the terminator in the Windows-owned allocation.
        let units = unsafe { slice::from_raw_parts(string.0, length) };
        String::from_utf16(units).map_err(|_| TransportError::MalformedToken)
    }

    fn from_token(token: &OwnedToken) -> Result<Self, TransportError> {
        let buffer = token.information(TokenGroups)?;
        let groups = buffer.as_token_groups()?;
        groups
            .iter()
            .find(|group| {
                group.Attributes & SE_GROUP_LOGON_ID.cast_unsigned()
                    == SE_GROUP_LOGON_ID.cast_unsigned()
            })
            .map(|group| Self::copy_from(group.Sid))
            .transpose()?
            .ok_or(TransportError::MissingLogonSid)
    }

    fn copy_from(source: PSID) -> Result<Self, TransportError> {
        // SAFETY: `source` comes from a validated token-information buffer.
        if unsafe { IsValidSid(source) } == 0 {
            return Err(TransportError::MalformedToken);
        }
        // SAFETY: `source` is a valid SID for the duration of this call.
        let length = unsafe { GetLengthSid(source) };
        let length_usize = usize::try_from(length).map_err(|_| TransportError::MalformedToken)?;
        let mut bytes = vec![0; length_usize];
        // SAFETY: the destination has exactly `length` writable bytes and source
        // remains valid until the token buffer is dropped.
        if unsafe { CopySid(length, bytes.as_mut_ptr().cast(), source) } == 0 {
            return Err(TransportError::windows("CopySid"));
        }
        Ok(Self(bytes.into_boxed_slice()))
    }

    pub(crate) fn as_psid(&self) -> PSID {
        self.0.as_ptr().cast_mut().cast()
    }

    pub(crate) fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

struct OwnedToken(HANDLE);

impl OwnedToken {
    fn current_process() -> Result<Self, TransportError> {
        let mut token = ptr::null_mut();
        // SAFETY: the pseudo process handle is valid and `token` is a writable output.
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw mut token) } == 0 {
            Err(TransportError::windows("OpenProcessToken"))
        } else {
            Ok(Self(token))
        }
    }

    fn current_thread() -> Result<Self, TransportError> {
        let mut token = ptr::null_mut();
        // SAFETY: the pseudo thread handle is valid and `token` is a writable output.
        if unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, 1, &raw mut token) } == 0 {
            Err(TransportError::windows("OpenThreadToken"))
        } else {
            Ok(Self(token))
        }
    }

    fn information(&self, class: i32) -> Result<TokenBuffer, TransportError> {
        let mut required = 0;
        // SAFETY: a null buffer with length zero is the documented sizing query.
        let first =
            unsafe { GetTokenInformation(self.0, class, ptr::null_mut(), 0, &raw mut required) };
        if first != 0 || required == 0 {
            return Err(TransportError::Windows {
                operation: "GetTokenInformation(size)",
                source: io::Error::last_os_error(),
            });
        }

        let words = usize::try_from(required)
            .map_err(|_| TransportError::MalformedToken)?
            .div_ceil(size_of::<usize>());
        let mut storage = vec![0_usize; words];
        let byte_capacity = storage
            .len()
            .checked_mul(size_of::<usize>())
            .and_then(|bytes| u32::try_from(bytes).ok())
            .ok_or(TransportError::MalformedToken)?;
        let mut written = required;
        // SAFETY: `storage` is aligned and writable for `byte_capacity` bytes.
        if unsafe {
            GetTokenInformation(
                self.0,
                class,
                storage.as_mut_ptr().cast::<c_void>(),
                byte_capacity,
                &raw mut written,
            )
        } == 0
        {
            return Err(TransportError::windows("GetTokenInformation"));
        }
        if written > byte_capacity {
            return Err(TransportError::MalformedToken);
        }
        Ok(TokenBuffer { storage, written })
    }
}

impl Drop for OwnedToken {
    fn drop(&mut self) {
        // SAFETY: `OwnedToken` uniquely owns the real token handle.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

struct TokenBuffer {
    storage: Vec<usize>,
    written: u32,
}

impl TokenBuffer {
    fn as_token_groups(&self) -> Result<&[SID_AND_ATTRIBUTES], TransportError> {
        let written = usize::try_from(self.written).map_err(|_| TransportError::MalformedToken)?;
        let groups_offset = offset_of!(TOKEN_GROUPS, Groups);
        if written < groups_offset {
            return Err(TransportError::MalformedToken);
        }
        let groups = self.storage.as_ptr().cast::<TOKEN_GROUPS>();
        // SAFETY: the token query initialized at least the fixed TOKEN_GROUPS header.
        let count = usize::try_from(unsafe { (*groups).GroupCount })
            .map_err(|_| TransportError::MalformedToken)?;
        let available = (written - groups_offset) / size_of::<SID_AND_ATTRIBUTES>();
        if count > available {
            return Err(TransportError::MalformedToken);
        }
        // SAFETY: `count` was bounded by the initialized buffer and the storage
        // alignment satisfies SID_AND_ATTRIBUTES.
        Ok(unsafe { slice::from_raw_parts(ptr::addr_of!((*groups).Groups).cast(), count) })
    }
}

struct LocalWideString(*mut u16);

impl Drop for LocalWideString {
    fn drop(&mut self) {
        // SAFETY: the pointer was allocated by ConvertSidToStringSidW with LocalAlloc.
        let _ = unsafe { LocalFree(self.0.cast()) };
    }
}
