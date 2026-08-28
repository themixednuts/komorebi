use std::collections::BTreeSet;
use std::mem::size_of;
use std::ptr;

use anyhow::{Result, bail};
use windows_sys::Win32::Foundation::{HWND, LPARAM, RECT, TRUE};
use windows_sys::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DwmFlush, DwmGetWindowAttribute};
use windows_sys::Win32::Graphics::Gdi::{DEVMODEW, ENUM_CURRENT_SETTINGS, EnumDisplaySettingsW};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetWindowDisplayAffinity, GetWindowRect, GetWindowTextLengthW,
    GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindowVisible,
};
use windows_sys::core::BOOL;

use crate::model::{DisplayMode, WindowCandidate};

pub fn current_display_mode() -> Result<DisplayMode> {
    read_display_mode(ENUM_CURRENT_SETTINGS)
}

pub fn available_display_modes() -> Result<Vec<DisplayMode>> {
    let current = current_display_mode()?;
    let mut modes = BTreeSet::new();
    let mut index = 0;
    while let Some(mode) = try_read_display_mode(index) {
        if mode.width_px == current.width_px && mode.height_px == current.height_px {
            modes.insert(mode);
        }
        index += 1;
    }
    Ok(modes.into_iter().collect())
}

pub fn visible_windows() -> Result<Vec<WindowCandidate>> {
    let mut windows = Vec::new();
    let succeeded = unsafe {
        EnumWindows(
            Some(collect_window),
            (&mut windows as *mut Vec<WindowCandidate>) as LPARAM,
        )
    };
    if succeeded == 0 {
        bail!("EnumWindows failed")
    }
    Ok(windows)
}

pub fn work_area() -> Result<RECT> {
    let mut area = RECT::default();
    let succeeded = unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::SystemParametersInfoW(
            windows_sys::Win32::UI::WindowsAndMessaging::SPI_GETWORKAREA,
            0,
            (&mut area as *mut RECT).cast(),
            0,
        )
    };
    if succeeded == 0 {
        bail!("SPI_GETWORKAREA failed")
    }
    Ok(area)
}

pub struct DisplayModeLease {
    original: DEVMODEW,
    changed: bool,
}

impl DisplayModeLease {
    pub fn switch_to(refresh_hz: u32) -> Result<Self> {
        let original = raw_display_mode(ENUM_CURRENT_SETTINGS)
            .ok_or_else(|| anyhow::anyhow!("current display mode unavailable"))?;
        if original.dmDisplayFrequency == refresh_hz {
            return Ok(Self {
                original,
                changed: false,
            });
        }
        let target = raw_modes().find(|mode| {
            mode.dmPelsWidth == original.dmPelsWidth
                && mode.dmPelsHeight == original.dmPelsHeight
                && mode.dmBitsPerPel == original.dmBitsPerPel
                && mode.dmDisplayFrequency == refresh_hz
        });
        let Some(target) = target else {
            bail!("refresh rate {refresh_hz} Hz is unavailable at the current resolution")
        };
        change_display_mode(&target, windows_sys::Win32::Graphics::Gdi::CDS_TEST)?;
        change_display_mode(&target, 0)?;
        let observed = current_display_mode()?;
        if observed.refresh_hz != refresh_hz {
            bail!(
                "display switch returned {refresh_hz} Hz but Windows reports {} Hz",
                observed.refresh_hz
            )
        }
        Ok(Self {
            original,
            changed: true,
        })
    }
}

impl Drop for DisplayModeLease {
    fn drop(&mut self) {
        if self.changed {
            let _ = change_display_mode(&self.original, 0);
        }
    }
}

fn read_display_mode(index: u32) -> Result<DisplayMode> {
    try_read_display_mode(index).ok_or_else(|| anyhow::anyhow!("display mode {index} unavailable"))
}

fn try_read_display_mode(index: u32) -> Option<DisplayMode> {
    let mode = raw_display_mode(index)?;
    Some(DisplayMode {
        width_px: mode.dmPelsWidth,
        height_px: mode.dmPelsHeight,
        refresh_hz: mode.dmDisplayFrequency,
        bits_per_pixel: mode.dmBitsPerPel,
    })
}

fn raw_display_mode(index: u32) -> Option<DEVMODEW> {
    let mut mode = DEVMODEW {
        dmSize: u16::try_from(size_of::<DEVMODEW>()).ok()?,
        ..Default::default()
    };
    let succeeded = unsafe { EnumDisplaySettingsW(ptr::null(), index, &mut mode) };
    (succeeded != 0).then_some(mode)
}

fn raw_modes() -> impl Iterator<Item = DEVMODEW> {
    (0..).map_while(raw_display_mode)
}

fn change_display_mode(mode: &DEVMODEW, flags: u32) -> Result<()> {
    let result = unsafe {
        windows_sys::Win32::Graphics::Gdi::ChangeDisplaySettingsExW(
            ptr::null(),
            mode,
            ptr::null_mut(),
            flags,
            ptr::null(),
        )
    };
    if result != windows_sys::Win32::Graphics::Gdi::DISP_CHANGE_SUCCESSFUL {
        bail!("ChangeDisplaySettingsExW failed with status {result}")
    }
    if flags == 0 {
        let flush = unsafe { DwmFlush() };
        if flush < 0 {
            bail!("DwmFlush after display change failed with HRESULT {flush:#x}")
        }
    }
    Ok(())
}

unsafe extern "system" fn collect_window(hwnd: HWND, state: LPARAM) -> BOOL {
    if unsafe { IsWindowVisible(hwnd) } == 0 || is_cloaked(hwnd) {
        return TRUE;
    }
    let mut rect = RECT::default();
    if unsafe { GetWindowRect(hwnd, &mut rect) } == 0
        || rect.right - rect.left < 100
        || rect.bottom - rect.top < 60
    {
        return TRUE;
    }

    let mut process_id = 0;
    unsafe { GetWindowThreadProcessId(hwnd, &mut process_id) };
    let mut capture_affinity = 0;
    unsafe { GetWindowDisplayAffinity(hwnd, &mut capture_affinity) };
    let class_utf16 = window_class(hwnd);
    let title_utf16 = window_title(hwnd);
    let candidate = WindowCandidate {
        handle: hwnd as isize,
        process_id,
        class_display: String::from_utf16_lossy(&class_utf16),
        title_display: String::from_utf16_lossy(&title_utf16),
        class_utf16,
        title_utf16,
        minimized: unsafe { IsIconic(hwnd) } != 0,
        capture_affinity,
    };
    let windows = unsafe { &mut *(state as *mut Vec<WindowCandidate>) };
    windows.push(candidate);
    TRUE
}

fn is_cloaked(hwnd: HWND) -> bool {
    let mut cloaked = 0u32;
    let result = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED as u32,
            (&mut cloaked as *mut u32).cast(),
            u32::try_from(size_of::<u32>()).unwrap_or(4),
        )
    };
    result >= 0 && cloaked != 0
}

fn window_class(hwnd: HWND) -> Vec<u16> {
    let mut value = vec![0u16; 256];
    let length = unsafe { GetClassNameW(hwnd, value.as_mut_ptr(), value.len() as i32) };
    value.truncate(length.max(0) as usize);
    value
}

fn window_title(hwnd: HWND) -> Vec<u16> {
    let length = unsafe { GetWindowTextLengthW(hwnd) };
    if length <= 0 {
        return Vec::new();
    }
    let mut value = vec![0u16; length as usize + 1];
    let copied = unsafe { GetWindowTextW(hwnd, value.as_mut_ptr(), value.len() as i32) };
    value.truncate(copied.max(0) as usize);
    value
}
