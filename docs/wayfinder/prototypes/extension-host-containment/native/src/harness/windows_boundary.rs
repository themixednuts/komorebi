use std::ffi::{OsStr, OsString, c_void};
use std::mem::size_of;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result, bail, ensure};
use uuid::Uuid;
use windows_sys::Win32::Foundation::{GENERIC_WRITE, LocalFree};
use windows_sys::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::Isolation::{
    CreateAppContainerProfile, DeleteAppContainerProfile, GetAppContainerFolderPath,
};
use windows_sys::Win32::Security::{
    DeriveCapabilitySidsFromName, PSECURITY_DESCRIPTOR, SECURITY_CAPABILITIES, SID_AND_ATTRIBUTES,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE,
    FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::System::IO::DeviceIoControl;
use windows_sys::Win32::System::Ioctl::FSCTL_SET_REPARSE_POINT;
use windows_sys::Win32::System::SystemServices::IO_REPARSE_TAG_MOUNT_POINT;
use windows_sys::Win32::System::SystemServices::SE_GROUP_ENABLED;
use windows_sys::Win32::System::Threading::{
    DeleteProcThreadAttributeList, InitializeProcThreadAttributeList,
    PROC_THREAD_ATTRIBUTE_ALL_APPLICATION_PACKAGES_POLICY,
    PROC_THREAD_ATTRIBUTE_CHILD_PROCESS_POLICY, PROC_THREAD_ATTRIBUTE_MITIGATION_POLICY,
    PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES, UpdateProcThreadAttribute,
};

use crate::protocol::RuntimeKind;
use crate::windows::OwnedHandle;
use crate::windows::{sid_to_string, wide};

use super::policy::ContainmentPolicy;

static PROFILE_CLEANUP_FAILED: AtomicBool = AtomicBool::new(false);

pub(super) struct AppContainerProfile {
    pub(super) name: String,
    name_wide: Vec<u16>,
    pub(super) sid: *mut c_void,
    pub(super) sid_string: String,
    pub(super) folder: PathBuf,
    pub(super) capabilities: CapabilitySet,
}

pub(super) struct CapabilitySet {
    pub(super) entries: Vec<SID_AND_ATTRIBUTES>,
    allocations: Vec<(*mut *mut c_void, u32)>,
}

impl CapabilitySet {
    fn derive(names: &[String]) -> Result<Self> {
        let mut result = Self {
            entries: Vec::new(),
            allocations: Vec::new(),
        };
        for name in names {
            let name = wide(name);
            let mut group_sids = null_mut();
            let mut group_count = 0_u32;
            let mut capability_sids = null_mut();
            let mut capability_count = 0_u32;
            // SAFETY: name is NUL-terminated and every output pointer/count is writable.
            if unsafe {
                DeriveCapabilitySidsFromName(
                    name.as_ptr(),
                    &raw mut group_sids,
                    &raw mut group_count,
                    &raw mut capability_sids,
                    &raw mut capability_count,
                )
            } == 0
            {
                return Err(std::io::Error::last_os_error()).context("derive LPAC capability SID");
            }
            for index in 0..capability_count as usize {
                // SAFETY: index is bounded by capability_count for the returned pointer array.
                let sid = unsafe { *capability_sids.add(index) };
                result.entries.push(SID_AND_ATTRIBUTES {
                    Sid: sid,
                    Attributes: u32::try_from(SE_GROUP_ENABLED)?,
                });
            }
            result.allocations.push((group_sids, group_count));
            result.allocations.push((capability_sids, capability_count));
        }
        Ok(result)
    }
}

impl Drop for CapabilitySet {
    fn drop(&mut self) {
        for (array, count) in self.allocations.drain(..) {
            if !array.is_null() {
                for index in 0..count as usize {
                    // SAFETY: DeriveCapabilitySidsFromName allocated each SID with LocalAlloc.
                    unsafe { LocalFree(*array.add(index)) };
                }
                // SAFETY: DeriveCapabilitySidsFromName allocated the pointer array with LocalAlloc.
                unsafe { LocalFree(array.cast()) };
            }
        }
    }
}

impl AppContainerProfile {
    pub(super) fn create(runtime: RuntimeKind, policy: &ContainmentPolicy) -> Result<Self> {
        let capabilities = CapabilitySet::derive(policy.compatibility_capabilities())?;
        let suffix = Uuid::new_v4().simple().to_string();
        let runtime_name = match runtime {
            RuntimeKind::Rust => "rust",
            RuntimeKind::LuaJit => "lua",
        };
        let name = format!("{}.{runtime_name}.{suffix}", policy.profile_prefix());
        let name_wide = wide(&name);
        let display = wide("Komorebi Wayfinder containment probe");
        let description = wide("Disposable LPAC extension-host prototype");
        let mut sid = null_mut();
        // SAFETY: strings are NUL-terminated, capabilities live through the call, and sid is writable.
        let result = unsafe {
            CreateAppContainerProfile(
                name_wide.as_ptr(),
                display.as_ptr(),
                description.as_ptr(),
                capabilities.entries.as_ptr(),
                u32::try_from(capabilities.entries.len())?,
                &raw mut sid,
            )
        };
        ensure!(
            result >= 0,
            "CreateAppContainerProfile failed: HRESULT {result:#x}"
        );
        // SAFETY: CreateAppContainerProfile returned a valid SID allocation.
        let sid_string = unsafe { sid_to_string(sid)? };
        let sid_wide = wide(&sid_string);
        let mut folder_wide = null_mut();
        // SAFETY: sid_wide is NUL-terminated and folder_wide is writable.
        let result = unsafe { GetAppContainerFolderPath(sid_wide.as_ptr(), &raw mut folder_wide) };
        if result < 0 {
            // SAFETY: CreateAppContainerProfile allocated sid with LocalAlloc.
            unsafe { LocalFree(sid) };
            bail!("GetAppContainerFolderPath failed: HRESULT {result:#x}");
        }
        let folder = PathBuf::from(read_wide(folder_wide));
        // SAFETY: GetAppContainerFolderPath documents CoTaskMemFree for this allocation.
        unsafe { CoTaskMemFree(folder_wide.cast()) };
        Ok(Self {
            name,
            name_wide,
            sid,
            sid_string,
            folder,
            capabilities,
        })
    }
}

impl Drop for AppContainerProfile {
    fn drop(&mut self) {
        // SAFETY: name remains NUL-terminated and sid is owned by this value.
        unsafe {
            LocalFree(self.sid);
            if std::env::var_os("WAYFINDER_RETAIN_PROFILE").is_none() {
                let result = DeleteAppContainerProfile(self.name_wide.as_ptr());
                if result < 0 {
                    PROFILE_CLEANUP_FAILED.store(true, Ordering::Release);
                    eprintln!(
                        "failed to delete profile {}: HRESULT {result:#x}",
                        self.name
                    );
                }
            } else {
                eprintln!(
                    "retained profile {} at {}",
                    self.name,
                    self.folder.display()
                );
            }
        }
    }
}

pub(super) fn profile_cleanup_succeeded() -> bool {
    !PROFILE_CLEANUP_FAILED.load(Ordering::Acquire)
}

pub(super) fn delete_profile(profile_name: &str, policy: &ContainmentPolicy) -> Result<()> {
    ensure!(
        profile_name.starts_with(&format!("{}.", policy.profile_prefix())),
        "refusing to delete a non-prototype AppContainer profile"
    );
    let profile_name = wide(profile_name);
    // SAFETY: profile_name is NUL-terminated and deletion is constrained by the checked prefix.
    let result = unsafe { DeleteAppContainerProfile(profile_name.as_ptr()) };
    ensure!(
        result >= 0,
        "DeleteAppContainerProfile failed: HRESULT {result:#x}"
    );
    Ok(())
}

pub(super) fn create_junction(link: &Path, target: &Path) -> Result<()> {
    std::fs::create_dir(link)
        .with_context(|| format!("create junction directory {}", link.display()))?;
    let canonical_target = std::fs::canonicalize(target)
        .with_context(|| format!("canonicalize junction target {}", target.display()))?;
    let (substitute, print_name) = junction_names(&canonical_target)?;
    let buffer = mount_point_buffer(&substitute, &print_name)?;
    let link_wide = wide(link.as_os_str());
    // SAFETY: link_wide is NUL-terminated; the directory exists and all other pointers are null.
    let directory = OwnedHandle::new(unsafe {
        CreateFileW(
            link_wide.as_ptr(),
            GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            null(),
            OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS,
            null_mut(),
        )
    })?;
    let mut returned = 0_u32;
    // SAFETY: directory is an owned reparse-point handle and buffer contains a complete
    // mount-point REPARSE_DATA_BUFFER for the advertised byte count.
    if unsafe {
        DeviceIoControl(
            directory.raw(),
            FSCTL_SET_REPARSE_POINT,
            buffer.as_ptr().cast(),
            u32::try_from(buffer.len())?,
            null_mut(),
            0,
            &raw mut returned,
            null_mut(),
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error())
            .context("set directory junction reparse point");
    }
    Ok(())
}

fn junction_names(canonical_target: &Path) -> Result<(OsString, OsString)> {
    const VERBATIM_PREFIX: &[u16] = &[b'\\' as u16, b'\\' as u16, b'?' as u16, b'\\' as u16];
    let canonical: Vec<_> = canonical_target.as_os_str().encode_wide().collect();
    let target = canonical
        .strip_prefix(VERBATIM_PREFIX)
        .context("canonical Windows path did not have a verbatim prefix")?;
    let mut substitute: Vec<_> = OsStr::new(r"\??\").encode_wide().collect();
    substitute.extend_from_slice(target);
    Ok((
        OsString::from_wide(&substitute),
        OsString::from_wide(target),
    ))
}

fn mount_point_buffer(substitute: &OsStr, print_name: &OsStr) -> Result<Vec<u8>> {
    let substitute: Vec<_> = substitute.encode_wide().collect();
    let print_name: Vec<_> = print_name.encode_wide().collect();
    let substitute_bytes = substitute
        .len()
        .checked_mul(2)
        .context("junction path overflow")?;
    let print_bytes = print_name
        .len()
        .checked_mul(2)
        .context("junction path overflow")?;
    let print_offset = substitute_bytes
        .checked_add(2)
        .context("junction path overflow")?;
    let path_bytes = print_offset
        .checked_add(print_bytes)
        .and_then(|bytes| bytes.checked_add(2))
        .context("junction path overflow")?;
    let data_bytes = 8_usize
        .checked_add(path_bytes)
        .context("junction buffer overflow")?;
    let mut buffer = vec![
        0_u8;
        8_usize
            .checked_add(data_bytes)
            .context("junction buffer overflow")?
    ];
    write_u32(&mut buffer, 0, IO_REPARSE_TAG_MOUNT_POINT);
    write_u16(&mut buffer, 4, u16::try_from(data_bytes)?);
    write_u16(&mut buffer, 8, 0);
    write_u16(&mut buffer, 10, u16::try_from(substitute_bytes)?);
    write_u16(&mut buffer, 12, u16::try_from(print_offset)?);
    write_u16(&mut buffer, 14, u16::try_from(print_bytes)?);
    for (index, unit) in substitute
        .into_iter()
        .chain(Some(0))
        .chain(print_name)
        .chain(Some(0))
        .enumerate()
    {
        write_u16(&mut buffer, 16 + index * 2, unit);
    }
    Ok(buffer)
}

fn write_u16(buffer: &mut [u8], offset: usize, value: u16) {
    buffer[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn write_u32(buffer: &mut [u8], offset: usize, value: u32) {
    buffer[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

pub(super) struct SecurityDescriptor(pub(super) PSECURITY_DESCRIPTOR);

impl SecurityDescriptor {
    pub(super) fn pipe_for(user_sid: &str, app_sid: &str) -> Result<(Self, String)> {
        let sddl_text = format!("D:P(A;;GA;;;SY)(A;;GA;;;{user_sid})(A;;GRGW;;;{app_sid})");
        let sddl = wide(&sddl_text);
        let mut descriptor = null_mut();
        // SAFETY: sddl is NUL-terminated and descriptor is writable.
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &raw mut descriptor,
                null_mut(),
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error()).context("build pipe security descriptor");
        }
        Ok((Self(descriptor), sddl_text))
    }
}

impl Drop for SecurityDescriptor {
    fn drop(&mut self) {
        // SAFETY: conversion API allocated this descriptor with LocalAlloc.
        unsafe { LocalFree(self.0) };
    }
}

pub(super) struct AttributeList {
    storage: Vec<u8>,
}

impl AttributeList {
    pub(super) fn create(
        capabilities: &SECURITY_CAPABILITIES,
        all_packages_policy: &u32,
        mitigation: &[u64; 2],
        child_policy: &u32,
    ) -> Result<Self> {
        let mut bytes = 0_usize;
        // SAFETY: null is the documented size-query form.
        unsafe { InitializeProcThreadAttributeList(null_mut(), 4, 0, &raw mut bytes) };
        ensure!(bytes > 0, "attribute-list size query failed");
        let mut result = Self {
            storage: vec![0_u8; bytes],
        };
        // SAFETY: storage is writable for the size returned by the query.
        if unsafe { InitializeProcThreadAttributeList(result.raw(), 4, 0, &raw mut bytes) } == 0 {
            return Err(std::io::Error::last_os_error()).context("initialize attribute list");
        }
        result.update(
            PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES,
            std::ptr::from_ref(capabilities).cast::<c_void>(),
            size_of::<SECURITY_CAPABILITIES>(),
        )?;
        result.update(
            PROC_THREAD_ATTRIBUTE_ALL_APPLICATION_PACKAGES_POLICY,
            std::ptr::from_ref(all_packages_policy).cast::<c_void>(),
            size_of::<u32>(),
        )?;
        result.update(
            PROC_THREAD_ATTRIBUTE_MITIGATION_POLICY,
            std::ptr::from_ref(mitigation).cast::<c_void>(),
            size_of::<[u64; 2]>(),
        )?;
        result.update(
            PROC_THREAD_ATTRIBUTE_CHILD_PROCESS_POLICY,
            std::ptr::from_ref(child_policy).cast::<c_void>(),
            size_of::<u32>(),
        )?;
        Ok(result)
    }

    pub(super) fn raw(
        &mut self,
    ) -> windows_sys::Win32::System::Threading::LPPROC_THREAD_ATTRIBUTE_LIST {
        self.storage.as_mut_ptr().cast()
    }

    fn update(&mut self, attribute: u32, value: *const c_void, size: usize) -> Result<()> {
        // SAFETY: list is initialized and values remain alive through process creation.
        if unsafe {
            UpdateProcThreadAttribute(
                self.raw(),
                0,
                attribute as usize,
                value,
                size,
                null_mut(),
                null(),
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error())
                .with_context(|| format!("set process attribute {attribute}"));
        }
        Ok(())
    }
}

impl Drop for AttributeList {
    fn drop(&mut self) {
        // SAFETY: successful construction initialized this list exactly once.
        unsafe { DeleteProcThreadAttributeList(self.raw()) };
    }
}

fn read_wide(pointer: *const u16) -> OsString {
    let mut length = 0;
    // SAFETY: caller only passes a Windows-allocated NUL-terminated string.
    unsafe {
        while *pointer.add(length) != 0 {
            length += 1;
        }
        OsString::from_wide(std::slice::from_raw_parts(pointer, length))
    }
}
