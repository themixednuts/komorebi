use std::mem::size_of;
use std::ptr;

use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::Foundation::ERROR_INSUFFICIENT_BUFFER;
use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::Security::CheckTokenMembershipEx;
use windows_sys::Win32::Security::CreateWellKnownSid;
use windows_sys::Win32::Security::GetSidSubAuthority;
use windows_sys::Win32::Security::GetSidSubAuthorityCount;
use windows_sys::Win32::Security::GetTokenInformation;
use windows_sys::Win32::Security::SECURITY_MAX_SID_SIZE;
use windows_sys::Win32::Security::TOKEN_GROUPS;
use windows_sys::Win32::Security::TOKEN_MANDATORY_LABEL;
use windows_sys::Win32::Security::TOKEN_QUERY;
use windows_sys::Win32::Security::TokenCapabilities;
use windows_sys::Win32::Security::TokenIntegrityLevel;
use windows_sys::Win32::Security::TokenIsAppContainer;
use windows_sys::Win32::Security::WinBuiltinAnyPackageSid;
use windows_sys::Win32::System::SystemServices::CTMF_INCLUDE_APPCONTAINER;
use windows_sys::Win32::System::SystemServices::SECURITY_MANDATORY_LOW_RID;
use windows_sys::Win32::System::Threading::GetCurrentProcess;
use windows_sys::Win32::System::Threading::OpenProcessToken;

use super::WorkerContainmentFailure;

pub(super) fn is_app_container(token: HANDLE) -> Result<bool, WorkerContainmentFailure> {
    query_token_bool(token, TokenIsAppContainer)
        .map_err(|_| WorkerContainmentFailure::AppContainerQueryUnavailable)
}

pub(super) fn is_lpac() -> Result<bool, WorkerContainmentFailure> {
    let mut sid = [0_u8; SECURITY_MAX_SID_SIZE as usize];
    let mut sid_size = SECURITY_MAX_SID_SIZE;
    let created = unsafe {
        // SAFETY: SID storage is writable and sized by SECURITY_MAX_SID_SIZE.
        CreateWellKnownSid(
            WinBuiltinAnyPackageSid,
            ptr::null_mut(),
            sid.as_mut_ptr().cast(),
            &raw mut sid_size,
        )
    };
    if created == 0 {
        return Err(WorkerContainmentFailure::LpacQueryUnavailable(
            last_error_code(),
        ));
    }
    let mut member = 0;
    let checked = unsafe {
        // SAFETY: the effective process token and generated SID are live; output is writable.
        CheckTokenMembershipEx(
            ptr::null_mut(),
            sid.as_mut_ptr().cast(),
            CTMF_INCLUDE_APPCONTAINER,
            &raw mut member,
        )
    };
    if checked == 0 {
        Err(WorkerContainmentFailure::LpacQueryUnavailable(
            last_error_code(),
        ))
    } else {
        Ok(member == 0)
    }
}

fn query_token_bool(
    token: HANDLE,
    class: windows_sys::Win32::Security::TOKEN_INFORMATION_CLASS,
) -> Result<bool, u32> {
    let mut value = 0_u32;
    let mut returned = 0_u32;
    let queried = unsafe {
        // SAFETY: output pointers match the selected scalar token-information contract.
        GetTokenInformation(
            token,
            class,
            (&raw mut value).cast(),
            u32::BITS / 8,
            &raw mut returned,
        )
    };
    if queried == 0 {
        Err(last_error_code())
    } else {
        Ok(value != 0)
    }
}

fn last_error_code() -> u32 {
    std::io::Error::last_os_error()
        .raw_os_error()
        .map_or(u32::MAX, i32::cast_unsigned)
}

pub(super) fn has_low_integrity(token: HANDLE) -> Result<bool, WorkerContainmentFailure> {
    let buffer = TokenInformation::query(
        token,
        TokenIntegrityLevel,
        WorkerContainmentFailure::IntegrityQueryUnavailable,
    )?;
    let label = unsafe {
        // SAFETY: query guarantees a TOKEN_MANDATORY_LABEL-compatible aligned buffer.
        &*buffer.as_ptr().cast::<TOKEN_MANDATORY_LABEL>()
    };
    let sid = label.Label.Sid;
    let count = unsafe {
        // SAFETY: TokenIntegrityLevel returns a valid mandatory-label SID.
        GetSidSubAuthorityCount(sid)
    };
    if count.is_null() {
        return Err(WorkerContainmentFailure::IntegrityQueryUnavailable);
    }
    let count = unsafe {
        // SAFETY: null was rejected above.
        *count
    };
    if count == 0 {
        return Err(WorkerContainmentFailure::IntegrityQueryUnavailable);
    }
    let rid = unsafe {
        // SAFETY: count is nonzero and the final sub-authority is the integrity RID.
        GetSidSubAuthority(sid, u32::from(count - 1))
    };
    if rid.is_null() {
        Err(WorkerContainmentFailure::IntegrityQueryUnavailable)
    } else {
        let rid = unsafe {
            // SAFETY: null was rejected above.
            *rid
        };
        Ok(rid == SECURITY_MANDATORY_LOW_RID as u32)
    }
}

pub(super) fn has_no_capabilities(token: HANDLE) -> Result<bool, WorkerContainmentFailure> {
    let buffer = TokenInformation::query(
        token,
        TokenCapabilities,
        WorkerContainmentFailure::CapabilitiesQueryUnavailable,
    )?;
    let groups = unsafe {
        // SAFETY: query guarantees a TOKEN_GROUPS-compatible aligned buffer.
        &*buffer.as_ptr().cast::<TOKEN_GROUPS>()
    };
    Ok(groups.GroupCount == 0)
}

pub(super) struct ProcessToken(HANDLE);

impl ProcessToken {
    pub(super) fn open() -> Result<Self, WorkerContainmentFailure> {
        let mut token = ptr::null_mut();
        let opened = unsafe {
            // SAFETY: current-process pseudo-handle is valid and output is writable.
            OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw mut token)
        };
        if opened == 0 || token.is_null() {
            Err(WorkerContainmentFailure::TokenUnavailable)
        } else {
            Ok(Self(token))
        }
    }

    pub(super) const fn handle(&self) -> HANDLE {
        self.0
    }
}

impl Drop for ProcessToken {
    fn drop(&mut self) {
        unsafe {
            // SAFETY: this wrapper uniquely owns the token handle.
            CloseHandle(self.0);
        }
    }
}

struct TokenInformation(Vec<usize>);

impl TokenInformation {
    fn query(
        token: HANDLE,
        class: windows_sys::Win32::Security::TOKEN_INFORMATION_CLASS,
        unavailable: WorkerContainmentFailure,
    ) -> Result<Self, WorkerContainmentFailure> {
        let mut bytes = 0_u32;
        unsafe {
            // SAFETY: a null buffer is the documented size query.
            GetTokenInformation(token, class, ptr::null_mut(), 0, &raw mut bytes);
        }
        if std::io::Error::last_os_error().raw_os_error()
            != Some(ERROR_INSUFFICIENT_BUFFER.cast_signed())
            || bytes == 0
        {
            return Err(unavailable);
        }
        let words = usize::try_from(bytes)
            .map_err(|_| unavailable)?
            .div_ceil(size_of::<usize>());
        let mut storage = vec![0; words];
        let queried = unsafe {
            // SAFETY: aligned storage was sized by the preceding query.
            GetTokenInformation(
                token,
                class,
                storage.as_mut_ptr().cast(),
                bytes,
                &raw mut bytes,
            )
        };
        if queried == 0 {
            Err(unavailable)
        } else {
            Ok(Self(storage))
        }
    }

    const fn as_ptr(&self) -> *const usize {
        self.0.as_ptr()
    }
}
