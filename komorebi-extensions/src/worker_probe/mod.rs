mod mitigation;
mod token;

use windows_sys::Win32::System::SystemServices::PROCESS_MITIGATION_CHILD_PROCESS_POLICY;
use windows_sys::Win32::System::SystemServices::PROCESS_MITIGATION_DYNAMIC_CODE_POLICY;
use windows_sys::Win32::System::SystemServices::PROCESS_MITIGATION_SYSTEM_CALL_DISABLE_POLICY;
use windows_sys::Win32::System::Threading::ProcessChildProcessPolicy;
use windows_sys::Win32::System::Threading::ProcessDynamicCodePolicy;
use windows_sys::Win32::System::Threading::ProcessSystemCallDisablePolicy;

use self::mitigation::is_job_contained;
use self::mitigation::mitigation_enabled;
use self::token::ProcessToken;
use self::token::has_low_integrity;
use self::token::has_no_capabilities;
use self::token::is_app_container;
use self::token::is_lpac;

/// Exact reason the worker rejected its own containment boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkerContainmentFailure {
    TokenUnavailable,
    NotAppContainer,
    NotLessPrivileged,
    NotLowIntegrity,
    HasCapabilities,
    ChildProcessesAllowed,
    Win32kEnabled,
    DynamicCodeAllowed,
    NotJobContained,
    AppContainerQueryUnavailable,
    LpacQueryUnavailable(u32),
    IntegrityQueryUnavailable,
    CapabilitiesQueryUnavailable,
    ChildPolicyQueryUnavailable,
    Win32kPolicyQueryUnavailable,
    DynamicCodePolicyQueryUnavailable,
    JobQueryUnavailable,
}

impl WorkerContainmentFailure {
    #[must_use]
    pub fn exit_code(self) -> i32 {
        match self {
            Self::TokenUnavailable => 10,
            Self::NotAppContainer => 11,
            Self::NotLessPrivileged => 12,
            Self::NotLowIntegrity => 13,
            Self::HasCapabilities => 14,
            Self::ChildProcessesAllowed => 15,
            Self::Win32kEnabled => 16,
            Self::DynamicCodeAllowed => 17,
            Self::NotJobContained => 18,
            Self::AppContainerQueryUnavailable => 19,
            Self::LpacQueryUnavailable(code) => match i32::try_from(code) {
                Ok(code) => 1_000_i32.saturating_add(code),
                Err(_) => i32::MAX,
            },
            Self::IntegrityQueryUnavailable => 21,
            Self::CapabilitiesQueryUnavailable => 22,
            Self::ChildPolicyQueryUnavailable => 23,
            Self::Win32kPolicyQueryUnavailable => 24,
            Self::DynamicCodePolicyQueryUnavailable => 25,
            Self::JobQueryUnavailable => 26,
        }
    }
}

/// Attests the complete boundary before any untrusted source is read.
pub fn run_worker_containment_probe() -> Result<(), WorkerContainmentFailure> {
    let token = ProcessToken::open()?;

    require(
        is_app_container(token.handle())?,
        WorkerContainmentFailure::NotAppContainer,
    )?;
    require(is_lpac()?, WorkerContainmentFailure::NotLessPrivileged)?;
    require(
        has_low_integrity(token.handle())?,
        WorkerContainmentFailure::NotLowIntegrity,
    )?;
    require(
        has_no_capabilities(token.handle())?,
        WorkerContainmentFailure::HasCapabilities,
    )?;
    require(
        mitigation_enabled::<PROCESS_MITIGATION_CHILD_PROCESS_POLICY>(
            ProcessChildProcessPolicy,
            WorkerContainmentFailure::ChildPolicyQueryUnavailable,
        )?,
        WorkerContainmentFailure::ChildProcessesAllowed,
    )?;
    require(
        mitigation_enabled::<PROCESS_MITIGATION_SYSTEM_CALL_DISABLE_POLICY>(
            ProcessSystemCallDisablePolicy,
            WorkerContainmentFailure::Win32kPolicyQueryUnavailable,
        )?,
        WorkerContainmentFailure::Win32kEnabled,
    )?;
    require(
        mitigation_enabled::<PROCESS_MITIGATION_DYNAMIC_CODE_POLICY>(
            ProcessDynamicCodePolicy,
            WorkerContainmentFailure::DynamicCodePolicyQueryUnavailable,
        )?,
        WorkerContainmentFailure::DynamicCodeAllowed,
    )?;
    require(
        is_job_contained()?,
        WorkerContainmentFailure::NotJobContained,
    )
}

fn require(
    condition: bool,
    failure: WorkerContainmentFailure,
) -> Result<(), WorkerContainmentFailure> {
    if condition { Ok(()) } else { Err(failure) }
}
