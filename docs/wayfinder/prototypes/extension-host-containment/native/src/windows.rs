use std::ffi::c_void;
use std::mem::{ManuallyDrop, size_of};
use std::ptr::null_mut;

use anyhow::{Context, Result, bail};
use windows_sys::Win32::Foundation::{CloseHandle, FreeLibrary, HANDLE, LocalFree};
use windows_sys::Win32::Security::Authorization::{ConvertSidToStringSidW, ConvertStringSidToSidW};
use windows_sys::Win32::Security::{
    EqualSid, GetTokenInformation, TOKEN_APPCONTAINER_INFORMATION, TOKEN_GROUPS, TOKEN_QUERY,
    TOKEN_USER, TokenAppContainerSid, TokenGroups, TokenIsAppContainer,
    TokenIsLessPrivilegedAppContainer, TokenUser,
};
use windows_sys::Win32::System::LibraryLoader::{
    GetProcAddress, LOAD_LIBRARY_SEARCH_APPLICATION_DIR, LOAD_LIBRARY_SEARCH_SYSTEM32,
    LOAD_LIBRARY_SEARCH_USER_DIRS, LoadLibraryExW, SetDefaultDllDirectories,
};
use windows_sys::Win32::System::Registry::{
    HKEY_CURRENT_USER, KEY_READ, RegCloseKey, RegOpenKeyExW,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentProcessId, OpenProcess, OpenProcessToken,
    PROCESS_QUERY_INFORMATION, PROCESS_VM_READ,
};

use crate::protocol::{ChildFacts, ExpectedOutcome, ObservedOutcome, ProbeOutcome};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenSecurityIdentity {
    pub app_container: bool,
    pub less_privileged_app_container: bool,
    pub package_sid: String,
}

pub struct OwnedHandle(HANDLE);

impl OwnedHandle {
    /// Takes ownership of a valid Windows handle.
    ///
    /// # Errors
    ///
    /// Returns an error for null or `INVALID_HANDLE_VALUE`.
    pub fn new(handle: HANDLE) -> Result<Self> {
        if handle.is_null() || handle == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
            bail!(std::io::Error::last_os_error());
        }
        Ok(Self(handle))
    }

    #[must_use]
    pub const fn raw(&self) -> HANDLE {
        self.0
    }

    #[must_use]
    pub fn into_raw(self) -> HANDLE {
        ManuallyDrop::new(self).0
    }
}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        // SAFETY: this type owns a valid, non-pseudo HANDLE and closes it exactly once.
        unsafe { CloseHandle(self.0) };
    }
}

#[must_use]
pub fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[must_use]
pub fn harden_dll_search() -> bool {
    // SAFETY: the flags are a documented valid combination and require no pointers.
    unsafe {
        SetDefaultDllDirectories(
            LOAD_LIBRARY_SEARCH_APPLICATION_DIR
                | LOAD_LIBRARY_SEARCH_SYSTEM32
                | LOAD_LIBRARY_SEARCH_USER_DIRS,
        ) != 0
    }
}

/// Reads the current child process's security and bootstrap facts.
///
/// # Errors
///
/// Returns an error when Windows does not expose a required token property.
pub fn current_child_facts(dll_search_hardened: bool) -> Result<ChildFacts> {
    // SAFETY: GetCurrentProcess returns a valid pseudo-handle for this process.
    let process = unsafe { GetCurrentProcess() };
    let identity = token_security_identity(process)?;
    let mut environment_keys: Vec<_> = std::env::vars_os()
        .filter_map(|(key, _)| key.into_string().ok())
        .collect();
    environment_keys.sort_unstable();
    Ok(ChildFacts {
        // SAFETY: GetCurrentProcessId takes no pointers and cannot fail.
        pid: unsafe { GetCurrentProcessId() },
        app_container: identity.app_container,
        less_privileged_app_container: identity.less_privileged_app_container,
        package_sid: identity.package_sid,
        dll_search_hardened,
        environment_keys,
    })
}

/// Independently reads the token identity of an owned process handle.
///
/// # Errors
///
/// Returns an error when the process token cannot be opened or queried.
pub fn process_token_identity(process: &OwnedHandle) -> Result<TokenSecurityIdentity> {
    token_security_identity(process.raw())
}

fn token_security_identity(process: HANDLE) -> Result<TokenSecurityIdentity> {
    let mut token = null_mut();
    // SAFETY: token is a valid out pointer and the requested access is query-only.
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &raw mut token) } == 0 {
        return Err(std::io::Error::last_os_error()).context("open process token");
    }
    let token = OwnedHandle::new(token)?;
    let app_container = token_u32(token.raw(), TokenIsAppContainer)? != 0;
    let less_privileged = token_u32(token.raw(), TokenIsLessPrivilegedAppContainer).map_or_else(
        |_| token_has_sid(token.raw(), "S-1-15-2-1").map(|has_ambient_group| !has_ambient_group),
        |value| Ok(value != 0),
    )?;
    let package_sid = token_app_container_sid(token.raw())?.unwrap_or_default();
    Ok(TokenSecurityIdentity {
        app_container,
        less_privileged_app_container: less_privileged,
        package_sid,
    })
}

fn token_u32(token: HANDLE, class: i32) -> Result<u32> {
    let mut value = 0_u32;
    let mut returned = 0_u32;
    // SAFETY: value is writable for exactly its advertised size; token is queryable.
    if unsafe {
        GetTokenInformation(
            token,
            class,
            (&raw mut value).cast::<c_void>(),
            u32::try_from(size_of::<u32>())?,
            &raw mut returned,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error())
            .with_context(|| format!("query token flag class {class}"));
    }
    Ok(value)
}

fn token_app_container_sid(token: HANDLE) -> Result<Option<String>> {
    let mut returned = 0_u32;
    // SAFETY: null with zero length is the documented size-query form.
    unsafe {
        GetTokenInformation(
            token,
            TokenAppContainerSid,
            null_mut(),
            0,
            &raw mut returned,
        )
    };
    anyhow::ensure!(
        returned >= u32::try_from(size_of::<TOKEN_APPCONTAINER_INFORMATION>())?,
        "invalid AppContainer token information size"
    );
    let words = (returned as usize).div_ceil(size_of::<usize>());
    let mut buffer = vec![0_usize; words];
    // SAFETY: buffer is writable for returned bytes; token is queryable.
    if unsafe {
        GetTokenInformation(
            token,
            TokenAppContainerSid,
            buffer.as_mut_ptr().cast(),
            returned,
            &raw mut returned,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error()).context("query AppContainer SID");
    }
    // SAFETY: the successful query populated the prefix as TOKEN_APPCONTAINER_INFORMATION.
    let info = unsafe { &*buffer.as_ptr().cast::<TOKEN_APPCONTAINER_INFORMATION>() };
    if info.TokenAppContainer.is_null() {
        return Ok(None);
    }
    // SAFETY: TOKEN_APPCONTAINER_INFORMATION owns a valid SID for the lifetime of buffer.
    unsafe { sid_to_string(info.TokenAppContainer) }.map(Some)
}

fn token_has_sid(token: HANDLE, string_sid: &str) -> Result<bool> {
    let string_sid = wide(string_sid);
    let mut sid = null_mut();
    // SAFETY: string_sid is NUL-terminated and sid is a writable out pointer.
    if unsafe { ConvertStringSidToSidW(string_sid.as_ptr(), &raw mut sid) } == 0 {
        return Err(std::io::Error::last_os_error()).context("parse membership SID");
    }
    let mut required = 0_u32;
    // SAFETY: null with zero length is the documented size-query form.
    unsafe { GetTokenInformation(token, TokenGroups, null_mut(), 0, &raw mut required) };
    if required < u32::try_from(size_of::<TOKEN_GROUPS>())? {
        // SAFETY: ConvertStringSidToSidW allocated sid with LocalAlloc.
        unsafe { LocalFree(sid) };
        bail!("invalid token groups information size");
    }
    let words = (required as usize).div_ceil(size_of::<usize>());
    let mut buffer = vec![0_usize; words];
    // SAFETY: buffer is writable for required bytes and token is queryable.
    let result = unsafe {
        GetTokenInformation(
            token,
            TokenGroups,
            buffer.as_mut_ptr().cast(),
            required,
            &raw mut required,
        )
    };
    if result == 0 {
        let error = std::io::Error::last_os_error();
        // SAFETY: ConvertStringSidToSidW allocated sid with LocalAlloc.
        unsafe { LocalFree(sid) };
        return Err(error).context("query token groups");
    }
    // SAFETY: GetTokenInformation populated the aligned buffer as TOKEN_GROUPS. GroupCount bounds
    // the variable-length SID_AND_ATTRIBUTES array that follows its fixed prefix.
    let member = unsafe {
        let groups = &*buffer.as_ptr().cast::<TOKEN_GROUPS>();
        let entries = std::ptr::addr_of!(groups.Groups)
            .cast::<windows_sys::Win32::Security::SID_AND_ATTRIBUTES>();
        (0..groups.GroupCount as usize).any(|index| EqualSid((*entries.add(index)).Sid, sid) != 0)
    };
    // SAFETY: ConvertStringSidToSidW allocated sid with LocalAlloc.
    unsafe { LocalFree(sid) };
    Ok(member)
}

/// Converts a valid Windows SID to its canonical string form.
///
/// # Safety
///
/// `sid` must point to a valid SID for the duration of this call.
///
/// # Errors
///
/// Returns an error when Windows rejects the SID or its UTF-16 form is invalid.
pub unsafe fn sid_to_string(sid: *mut c_void) -> Result<String> {
    let mut string_sid = null_mut();
    // SAFETY: sid is supplied by Windows token/profile APIs; string_sid is an out pointer.
    if unsafe { ConvertSidToStringSidW(sid, &raw mut string_sid) } == 0 {
        return Err(std::io::Error::last_os_error()).context("convert SID to string");
    }
    let mut length = 0;
    // SAFETY: ConvertSidToStringSidW returned a NUL-terminated allocation.
    unsafe {
        while *string_sid.add(length) != 0 {
            length += 1;
        }
    }
    // SAFETY: length was found within the API-owned NUL-terminated allocation.
    let result = String::from_utf16(unsafe { std::slice::from_raw_parts(string_sid, length) })
        .context("decode SID")?;
    // SAFETY: this allocation is documented to be released with LocalFree.
    unsafe { LocalFree(string_sid.cast()) };
    Ok(result)
}

/// Returns the canonical SID of the user running the harness.
///
/// # Errors
///
/// Returns an error when the current process token cannot be queried.
pub fn current_user_sid() -> Result<String> {
    // SAFETY: GetCurrentProcess returns a valid pseudo-handle for this process.
    let process = unsafe { GetCurrentProcess() };
    let mut token = null_mut();
    // SAFETY: token is a valid out pointer and the requested access is query-only.
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &raw mut token) } == 0 {
        return Err(std::io::Error::last_os_error()).context("open current process token");
    }
    let token = OwnedHandle::new(token)?;
    let mut required = 0_u32;
    // SAFETY: a null buffer with zero length is the documented size-query form.
    unsafe {
        GetTokenInformation(token.raw(), TokenUser, null_mut(), 0, &raw mut required);
    }
    anyhow::ensure!(
        required >= u32::try_from(size_of::<TOKEN_USER>())?,
        "invalid token user size"
    );
    let words = (required as usize).div_ceil(size_of::<usize>());
    let mut buffer = vec![0_usize; words];
    // SAFETY: buffer is writable for required bytes and token is queryable.
    if unsafe {
        GetTokenInformation(
            token.raw(),
            TokenUser,
            buffer.as_mut_ptr().cast(),
            required,
            &raw mut required,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error()).context("query current user SID");
    }
    // SAFETY: GetTokenInformation populated the buffer with a TOKEN_USER structure.
    let user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
    // SAFETY: TOKEN_USER owns a valid SID for the lifetime of buffer.
    unsafe { sid_to_string(user.User.Sid) }
}

pub fn registry_probe() -> ProbeOutcome {
    let subkey = wide("Software");
    let mut key = null_mut();
    // SAFETY: subkey is NUL-terminated and key is a valid out pointer.
    let status = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            0,
            KEY_READ,
            &raw mut key,
        )
    };
    if status == 0 {
        // SAFETY: a successful RegOpenKeyExW returned an owned HKEY.
        unsafe { RegCloseKey(key) };
        probe_allowed("registry_hkcu", ExpectedOutcome::Denied)
    } else {
        probe_denied(
            "registry_hkcu",
            ExpectedOutcome::Denied,
            i32::try_from(status).ok(),
        )
    }
}

#[must_use]
pub fn parent_process_probe(parent_pid: u32) -> ProbeOutcome {
    // SAFETY: OpenProcess validates the PID and returns either a handle or null.
    let handle = unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, 0, parent_pid) };
    if handle.is_null() {
        probe_denied(
            "parent_process_vm_read",
            ExpectedOutcome::Denied,
            std::io::Error::last_os_error().raw_os_error(),
        )
    } else {
        // SAFETY: successful OpenProcess returned an owned handle.
        unsafe { CloseHandle(handle) };
        probe_allowed("parent_process_vm_read", ExpectedOutcome::Denied)
    }
}

#[must_use]
pub fn clipboard_probe() -> ProbeOutcome {
    type OpenClipboardFn = unsafe extern "system" fn(*mut c_void) -> i32;
    type CloseClipboardFn = unsafe extern "system" fn() -> i32;

    let user32 = wide("user32.dll");
    // SAFETY: filename is NUL-terminated and the search is constrained to System32.
    let module =
        unsafe { LoadLibraryExW(user32.as_ptr(), null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32) };
    if module.is_null() {
        return probe_denied(
            "clipboard_open",
            ExpectedOutcome::Denied,
            std::io::Error::last_os_error().raw_os_error(),
        );
    }
    // SAFETY: module is valid and names are static NUL-terminated ASCII.
    let (open, close) = unsafe {
        (
            GetProcAddress(module, c"OpenClipboard".as_ptr().cast()),
            GetProcAddress(module, c"CloseClipboard".as_ptr().cast()),
        )
    };
    let outcome = if let (Some(open), Some(close)) = (open, close) {
        // SAFETY: GetProcAddress returned the documented exports with these exact signatures.
        let open: OpenClipboardFn = unsafe { std::mem::transmute(open) };
        // SAFETY: GetProcAddress returned the documented exports with these exact signatures.
        let close: CloseClipboardFn = unsafe { std::mem::transmute(close) };
        // SAFETY: null is a permitted clipboard owner.
        if unsafe { open(null_mut()) } != 0 {
            // SAFETY: this balances the successful OpenClipboard call.
            unsafe { close() };
            probe_allowed("clipboard_open", ExpectedOutcome::Denied)
        } else {
            probe_denied(
                "clipboard_open",
                ExpectedOutcome::Denied,
                std::io::Error::last_os_error().raw_os_error(),
            )
        }
    } else {
        ProbeOutcome {
            name: "clipboard_open".to_owned(),
            expected: ExpectedOutcome::Denied,
            observed: ObservedOutcome::Unavailable {
                reason: "user32 clipboard exports unavailable".to_owned(),
            },
        }
    };
    // SAFETY: module is an owned LoadLibraryExW reference.
    unsafe { FreeLibrary(module) };
    outcome
}

#[must_use]
pub fn probe_allowed(name: &str, expected: ExpectedOutcome) -> ProbeOutcome {
    ProbeOutcome {
        name: name.to_owned(),
        expected,
        observed: ObservedOutcome::Allowed,
    }
}

#[must_use]
pub fn probe_denied(name: &str, expected: ExpectedOutcome, os_error: Option<i32>) -> ProbeOutcome {
    ProbeOutcome {
        name: name.to_owned(),
        expected,
        observed: ObservedOutcome::Denied { os_error },
    }
}
