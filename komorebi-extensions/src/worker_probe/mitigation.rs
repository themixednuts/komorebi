use std::mem::size_of;
use std::ptr;

use windows_sys::Win32::System::JobObjects::IsProcessInJob;
use windows_sys::Win32::System::Threading::GetCurrentProcess;
use windows_sys::Win32::System::Threading::GetProcessMitigationPolicy;

use super::WorkerContainmentFailure;

const POLICY_ENABLED: u32 = 1;

pub(super) fn mitigation_enabled<T>(
    policy: windows_sys::Win32::System::Threading::PROCESS_MITIGATION_POLICY,
    unavailable: WorkerContainmentFailure,
) -> Result<bool, WorkerContainmentFailure> {
    let mut value = vec![0_u8; size_of::<T>()];
    let queried = unsafe {
        // SAFETY: buffer length matches the policy structure selected by the caller.
        GetProcessMitigationPolicy(
            GetCurrentProcess(),
            policy,
            value.as_mut_ptr().cast(),
            value.len(),
        )
    };
    if queried == 0 {
        return Err(unavailable);
    }
    let flags = unsafe {
        // SAFETY: every queried policy here starts with its u32 Flags union member.
        value.as_ptr().cast::<u32>().read_unaligned()
    };
    Ok(flags & POLICY_ENABLED != 0)
}

pub(super) fn is_job_contained() -> Result<bool, WorkerContainmentFailure> {
    let mut contained = 0;
    let queried = unsafe {
        // SAFETY: current-process pseudo-handle is valid and output is writable.
        IsProcessInJob(GetCurrentProcess(), ptr::null_mut(), &raw mut contained)
    };
    if queried == 0 {
        Err(WorkerContainmentFailure::JobQueryUnavailable)
    } else {
        Ok(contained != 0)
    }
}
