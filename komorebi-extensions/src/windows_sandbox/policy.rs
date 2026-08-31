use std::ptr;

use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::Security::SECURITY_CAPABILITIES;
use windows_sys::Win32::System::Threading::PROC_THREAD_ATTRIBUTE_ALL_APPLICATION_PACKAGES_POLICY;
use windows_sys::Win32::System::Threading::PROC_THREAD_ATTRIBUTE_CHILD_PROCESS_POLICY;
use windows_sys::Win32::System::Threading::PROC_THREAD_ATTRIBUTE_HANDLE_LIST;
use windows_sys::Win32::System::Threading::PROC_THREAD_ATTRIBUTE_MITIGATION_POLICY;
use windows_sys::Win32::System::Threading::PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES;
use windows_sys::Win32::System::WindowsProgramming::PROCESS_CREATION_ALL_APPLICATION_PACKAGES_OPT_OUT;
use windows_sys::Win32::System::WindowsProgramming::PROCESS_CREATION_CHILD_PROCESS_RESTRICTED;

use super::LpacLaunchError;
use super::profile::AppContainerSid;
use super::resources::ProcessAttributes;

const BASE_ATTRIBUTE_COUNT: u32 = 4;
// Public processthreadsapi.h SDK macros are C shift expressions, so windows-rs metadata cannot
// generate them as Rust constants.
const STRICT_HANDLE_CHECKS_ALWAYS_ON: u64 = 1 << 24;
const WIN32K_SYSTEM_CALL_DISABLE_ALWAYS_ON: u64 = 1 << 28;
const EXTENSION_POINT_DISABLE_ALWAYS_ON: u64 = 1 << 32;
const PROHIBIT_DYNAMIC_CODE_ALWAYS_ON: u64 = 1 << 36;
const CREATION_MITIGATIONS: u64 = STRICT_HANDLE_CHECKS_ALWAYS_ON
    | WIN32K_SYSTEM_CALL_DISABLE_ALWAYS_ON
    | EXTENSION_POINT_DISABLE_ALWAYS_ON
    | PROHIBIT_DYNAMIC_CODE_ALWAYS_ON;

pub(super) fn with_process_policy<T>(
    sid: &AppContainerSid,
    inherited_handles: Option<&[HANDLE; 2]>,
    create: impl FnOnce(&mut ProcessAttributes) -> Result<T, LpacLaunchError>,
) -> Result<T, LpacLaunchError> {
    let security_capabilities = SECURITY_CAPABILITIES {
        AppContainerSid: sid.as_ptr(),
        Capabilities: ptr::null_mut(),
        CapabilityCount: 0,
        Reserved: 0,
    };
    let all_packages_policy = PROCESS_CREATION_ALL_APPLICATION_PACKAGES_OPT_OUT;
    let child_process_policy = PROCESS_CREATION_CHILD_PROCESS_RESTRICTED;
    let attribute_count = BASE_ATTRIBUTE_COUNT + u32::from(inherited_handles.is_some());
    let mut attributes = ProcessAttributes::new(attribute_count)?;
    attributes.update(
        PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES,
        &security_capabilities,
    )?;
    attributes.update(
        PROC_THREAD_ATTRIBUTE_ALL_APPLICATION_PACKAGES_POLICY,
        &all_packages_policy,
    )?;
    attributes.update(
        PROC_THREAD_ATTRIBUTE_CHILD_PROCESS_POLICY,
        &child_process_policy,
    )?;
    attributes.update(
        PROC_THREAD_ATTRIBUTE_MITIGATION_POLICY,
        &CREATION_MITIGATIONS,
    )?;
    if let Some(handles) = inherited_handles {
        attributes.update(PROC_THREAD_ATTRIBUTE_HANDLE_LIST, handles)?;
    }
    create(&mut attributes)
}
