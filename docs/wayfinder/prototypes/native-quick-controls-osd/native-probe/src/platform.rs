use std::mem::MaybeUninit;

use anyhow::{Context, Result};
use serde::Serialize;
use windows::Win32::Storage::Packaging::Appx::GetCurrentPackageFullName;
use windows::Win32::System::Power::{
    GetPwrCapabilities, GetSystemPowerStatus, SYSTEM_POWER_CAPABILITIES, SYSTEM_POWER_STATUS,
};

const APPMODEL_ERROR_NO_PACKAGE: u32 = 15_700;

#[derive(Debug, Serialize)]
pub struct MachineObservation {
    pub architecture: &'static str,
    pub package_identity: PackageIdentity,
    pub power: PowerObservation,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum PackageIdentity {
    Present,
    Absent,
    Unavailable { code: u32 },
}

#[derive(Debug, Serialize)]
pub struct PowerObservation {
    pub ac_line_status: u8,
    pub battery_flag: u8,
    pub battery_percent: u8,
    pub supported_sleep_states: Vec<&'static str>,
    pub hiberfile_present: bool,
}

pub fn observe_machine() -> Result<MachineObservation> {
    Ok(MachineObservation {
        architecture: std::env::consts::ARCH,
        package_identity: package_identity(),
        power: power().context("query power capabilities")?,
    })
}

fn package_identity() -> PackageIdentity {
    let mut length = 0;
    // SAFETY: a null output buffer with a zero length is the documented sizing query.
    let status = unsafe { GetCurrentPackageFullName(&raw mut length, None) };
    match status.0 {
        APPMODEL_ERROR_NO_PACKAGE => PackageIdentity::Absent,
        0 | 122 => PackageIdentity::Present,
        code => PackageIdentity::Unavailable { code },
    }
}

fn power() -> Result<PowerObservation> {
    let mut status = MaybeUninit::<SYSTEM_POWER_STATUS>::uninit();
    // SAFETY: the pointer names writable storage for the complete output structure.
    unsafe { GetSystemPowerStatus(status.as_mut_ptr()) }.context("GetSystemPowerStatus")?;
    // SAFETY: success initialized the complete structure.
    let status = unsafe { status.assume_init() };

    let mut capabilities = MaybeUninit::<SYSTEM_POWER_CAPABILITIES>::uninit();
    // SAFETY: the pointer names writable storage for the complete output structure.
    if !unsafe { GetPwrCapabilities(capabilities.as_mut_ptr()) } {
        return Err(std::io::Error::last_os_error()).context("GetPwrCapabilities");
    }
    // SAFETY: success initialized the complete structure.
    let capabilities = unsafe { capabilities.assume_init() };

    let supported_sleep_states = [
        (capabilities.SystemS1, "s1"),
        (capabilities.SystemS2, "s2"),
        (capabilities.SystemS3, "s3"),
        (capabilities.SystemS4, "s4"),
    ]
    .into_iter()
    .filter_map(|(supported, state)| supported.then_some(state))
    .collect();

    Ok(PowerObservation {
        ac_line_status: status.ACLineStatus,
        battery_flag: status.BatteryFlag,
        battery_percent: status.BatteryLifePercent,
        supported_sleep_states,
        hiberfile_present: capabilities.HiberFilePresent,
    })
}
