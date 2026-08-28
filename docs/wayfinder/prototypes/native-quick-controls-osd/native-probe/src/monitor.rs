use std::cell::RefCell;

use anyhow::{Context, Result};
use serde::Serialize;
use windows::Win32::Devices::Display::{
    DestroyPhysicalMonitors, GetMonitorBrightness, GetMonitorCapabilities,
    GetNumberOfPhysicalMonitorsFromHMONITOR, GetPhysicalMonitorsFromHMONITOR, MC_CAPS_BRIGHTNESS,
    PHYSICAL_MONITOR,
};
use windows::Win32::Foundation::{LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, HDC, HMONITOR};
use windows_core::BOOL;

use crate::native_text::NativeText;

thread_local! {
    static LOGICAL_MONITORS: RefCell<Vec<HMONITOR>> = const { RefCell::new(Vec::new()) };
}

#[derive(Debug, Serialize)]
pub struct MonitorProbe {
    pub description: NativeText,
    pub brightness_capability: bool,
    pub brightness: Option<Brightness>,
    pub brightness_error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Brightness {
    pub minimum: u32,
    pub current: u32,
    pub maximum: u32,
}

struct PhysicalMonitors(Vec<PHYSICAL_MONITOR>);

impl Drop for PhysicalMonitors {
    fn drop(&mut self) {
        // SAFETY: the slice contains the live handles returned together by Windows.
        if let Err(error) = unsafe { DestroyPhysicalMonitors(&self.0) } {
            eprintln!("failed to destroy physical-monitor handles: {error}");
        }
    }
}

unsafe extern "system" fn collect_monitor(
    monitor: HMONITOR,
    _dc: HDC,
    _rect: *mut RECT,
    _data: LPARAM,
) -> BOOL {
    LOGICAL_MONITORS.with_borrow_mut(|monitors| monitors.push(monitor));
    true.into()
}

pub fn observe() -> Result<Vec<MonitorProbe>> {
    LOGICAL_MONITORS.with_borrow_mut(Vec::clear);
    // SAFETY: the callback remains live for the synchronous enumeration and stores copied handles.
    if !unsafe { EnumDisplayMonitors(None, None, Some(collect_monitor), LPARAM(0)) }.as_bool() {
        return Err(std::io::Error::last_os_error()).context("EnumDisplayMonitors");
    }

    LOGICAL_MONITORS.with_borrow(|monitors| {
        let groups = monitors
            .iter()
            .copied()
            .map(observe_logical_monitor)
            .collect::<Result<Vec<_>>>()?;
        Ok(groups.into_iter().flatten().collect())
    })
}

fn observe_logical_monitor(monitor: HMONITOR) -> Result<Vec<MonitorProbe>> {
    let mut count = 0;
    // SAFETY: count points to writable storage and monitor came from EnumDisplayMonitors.
    unsafe { GetNumberOfPhysicalMonitorsFromHMONITOR(monitor, &raw mut count) }
        .context("GetNumberOfPhysicalMonitorsFromHMONITOR")?;
    let count = usize::try_from(count).context("physical monitor count exceeds usize")?;
    let mut monitors = vec![PHYSICAL_MONITOR::default(); count];
    // SAFETY: the output slice has the count reported by Windows for this logical monitor.
    unsafe { GetPhysicalMonitorsFromHMONITOR(monitor, &mut monitors) }
        .context("GetPhysicalMonitorsFromHMONITOR")?;
    let monitors = PhysicalMonitors(monitors);

    Ok(monitors.0.iter().map(observe_physical_monitor).collect())
}

fn observe_physical_monitor(monitor: &PHYSICAL_MONITOR) -> MonitorProbe {
    // SAFETY: PHYSICAL_MONITOR is packed; an unaligned copy avoids borrowing its array field.
    let raw_description =
        unsafe { std::ptr::addr_of!(monitor.szPhysicalMonitorDescription).read_unaligned() };
    let description_utf16 = raw_description
        .iter()
        .copied()
        .take_while(|unit| *unit != 0)
        .collect::<Vec<_>>();
    let mut capabilities = 0;
    let mut temperatures = 0;
    // SAFETY: the handle is live while PhysicalMonitors owns the containing array.
    let has_capabilities = unsafe {
        GetMonitorCapabilities(
            monitor.hPhysicalMonitor,
            &raw mut capabilities,
            &raw mut temperatures,
        ) != 0
    };
    let brightness_capability = has_capabilities && capabilities & MC_CAPS_BRIGHTNESS != 0;
    let (brightness, brightness_error) = if brightness_capability {
        let mut minimum = 0;
        let mut current = 0;
        let mut maximum = 0;
        // SAFETY: the handle is live and all output pointers name writable storage.
        if unsafe {
            GetMonitorBrightness(
                monitor.hPhysicalMonitor,
                &raw mut minimum,
                &raw mut current,
                &raw mut maximum,
            )
        } != 0
        {
            (
                Some(Brightness {
                    minimum,
                    current,
                    maximum,
                }),
                None,
            )
        } else {
            (None, Some(std::io::Error::last_os_error().to_string()))
        }
    } else {
        (None, None)
    };

    MonitorProbe {
        description: NativeText::from(description_utf16),
        brightness_capability,
        brightness,
        brightness_error,
    }
}
