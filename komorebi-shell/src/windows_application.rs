use std::ffi::c_void;
use std::mem::size_of;

use thiserror::Error;
use windows::Win32::System::Com::COINIT_MULTITHREADED;
use windows::Win32::System::Com::CoInitializeEx;
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::Com::CoUninitialize;
use windows::Win32::UI::Shell::BHID_EnumItems;
use windows::Win32::UI::Shell::IEnumShellItems;
use windows::Win32::UI::Shell::IShellItem;
use windows::Win32::UI::Shell::SEE_MASK_FLAG_NO_UI;
use windows::Win32::UI::Shell::SHCreateItemFromParsingName;
use windows::Win32::UI::Shell::SHELLEXECUTEINFOW;
use windows::Win32::UI::Shell::SIGDN_DESKTOPABSOLUTEPARSING;
use windows::Win32::UI::Shell::SIGDN_NORMALDISPLAY;
use windows::Win32::UI::Shell::ShellExecuteExW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::PCWSTR;
use windows::core::PWSTR;

use crate::ApplicationCatalog;
use crate::ApplicationDescriptor;
use crate::ApplicationId;
use crate::ApplicationLaunchFailure;
use crate::ApplicationLauncher;

const APPS_FOLDER: &[u16] = &[
    115, 104, 101, 108, 108, 58, 65, 112, 112, 115, 70, 111, 108, 100, 101, 114, 0,
];

/// Enumerates the public Windows `AppsFolder` namespace off the async executor.
pub async fn discover_installed_applications()
-> Result<ApplicationCatalog, ApplicationDiscoveryError> {
    tokio::task::spawn_blocking(discover_blocking)
        .await
        .map_err(ApplicationDiscoveryError::Worker)?
}

fn discover_blocking() -> Result<ApplicationCatalog, ApplicationDiscoveryError> {
    let _apartment = ComApartment::initialize()?;
    // SAFETY: `APPS_FOLDER` is statically NUL-terminated and windows-rs owns
    // the requested interface's Release call.
    let folder: IShellItem = unsafe {
        SHCreateItemFromParsingName(
            PCWSTR(APPS_FOLDER.as_ptr()),
            None::<&windows::Win32::System::Com::IBindCtx>,
        )?
    };
    // SAFETY: the folder and handler ID remain valid for the call.
    let items: IEnumShellItems = unsafe { folder.BindToHandler(None, &BHID_EnumItems)? };
    let mut applications = Vec::new();

    loop {
        let mut slot = [None];
        let mut fetched = 0;
        // SAFETY: `slot` and `fetched` remain live and writable for the call.
        unsafe { items.Next(&mut slot, Some(&raw mut fetched))? };
        if fetched == 0 {
            break;
        }
        let Some(item) = slot[0].take() else {
            return Err(ApplicationDiscoveryError::MissingItem);
        };
        let name = shell_string(&item, SIGDN_NORMALDISPLAY)?;
        let parsing_name = shell_string(&item, SIGDN_DESKTOPABSOLUTEPARSING)?;
        let Some(id) = ApplicationId::from_utf16(parsing_name) else {
            return Err(ApplicationDiscoveryError::InvalidIdentity);
        };
        applications.push(ApplicationDescriptor::new(
            id,
            String::from_utf16_lossy(&name),
        ));
    }

    Ok(ApplicationCatalog::new(applications))
}

fn shell_string(
    item: &IShellItem,
    kind: windows::Win32::UI::Shell::SIGDN,
) -> Result<Vec<u16>, windows::core::Error> {
    // SAFETY: the returned allocation belongs to the COM task allocator and is
    // copied before `CoTaskMemFree` runs.
    let value = unsafe { item.GetDisplayName(kind)? };
    // SAFETY: successful `GetDisplayName` returns a live NUL-terminated task
    // allocation, and it remains allocated until the free below.
    let owned = unsafe { copy_task_string(value) };
    // SAFETY: `value` was allocated by this Shell call's COM task allocator.
    unsafe { CoTaskMemFree(Some(value.0.cast::<c_void>())) };
    Ok(owned)
}

unsafe fn copy_task_string(value: PWSTR) -> Vec<u16> {
    let mut length = 0;
    // SAFETY: Shell returned a valid NUL-terminated task-allocated string.
    while unsafe { *value.0.add(length) } != 0 {
        length += 1;
    }
    // SAFETY: the scan established that `length` initialized code units precede
    // the terminator and the allocation remains owned by the caller.
    unsafe { std::slice::from_raw_parts(value.0, length) }.to_vec()
}

struct ComApartment;

impl ComApartment {
    fn initialize() -> Result<Self, windows::core::Error> {
        // SAFETY: this worker balances every successful initialization in Drop.
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok()? };
        Ok(Self)
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        // SAFETY: this instance exists only after successful `CoInitializeEx`.
        unsafe { CoUninitialize() };
    }
}

#[derive(Clone, Copy, Debug)]
pub struct WindowsApplicationLauncher;

impl ApplicationLauncher for WindowsApplicationLauncher {
    async fn launch(&self, id: ApplicationId) -> Result<(), ApplicationLaunchFailure> {
        tokio::task::spawn_blocking(move || launch_blocking(&id))
            .await
            .map_err(|error| ApplicationLaunchFailure::new(error.to_string()))?
    }
}

fn launch_blocking(id: &ApplicationId) -> Result<(), ApplicationLaunchFailure> {
    let operand = id.nul_terminated();
    let structure_size = u32::try_from(size_of::<SHELLEXECUTEINFOW>())
        .map_err(|_| ApplicationLaunchFailure::new("ShellExecuteExW structure size overflowed"))?;
    let mut execution = SHELLEXECUTEINFOW {
        cbSize: structure_size,
        fMask: SEE_MASK_FLAG_NO_UI,
        lpFile: PCWSTR(operand.as_ptr()),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    // SAFETY: all pointers refer to live, NUL-terminated buffers and the
    // structure advertises its exact checked size.
    unsafe { ShellExecuteExW(&raw mut execution) }
        .map_err(|error| ApplicationLaunchFailure::native(error.code().0, error.message()))
}

#[derive(Debug, Error)]
pub enum ApplicationDiscoveryError {
    #[error("AppsFolder enumeration worker failed: {0}")]
    Worker(tokio::task::JoinError),
    #[error("Windows Shell discovery failed: {0}")]
    Shell(#[from] windows::core::Error),
    #[error("AppsFolder returned a count without an item")]
    MissingItem,
    #[error("AppsFolder returned an empty or interior-NUL parsing identity")]
    InvalidIdentity,
}
