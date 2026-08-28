use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::mem::size_of;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, bail};
use windows::Win32::Foundation::{CloseHandle, FILETIME, HWND, POINT, RECT, WAIT_OBJECT_0};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromPoint,
};
use windows::Win32::System::Threading::{
    GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
    PROCESS_TERMINATE, TerminateProcess, WaitForSingleObject,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetShellWindow, GetWindowRect, GetWindowThreadProcessId, IsWindowVisible,
};

use crate::model::{Rect, ShellIdentity};

struct OwnedHandle(windows::Win32::Foundation::HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        // SAFETY: `OwnedHandle` is constructed only from a successful `OpenProcess` call and owns
        // exactly one close of that handle.
        if let Err(error) = unsafe { CloseHandle(self.0) } {
            eprintln!("close process handle: {error}");
        }
    }
}

/// Returns the current Explorer process generation.
///
/// # Errors
///
/// Returns an error when the Shell window or its creation time cannot be queried.
pub fn shell_identity() -> anyhow::Result<ShellIdentity> {
    // SAFETY: `GetShellWindow` has no preconditions.
    let hwnd = unsafe { GetShellWindow() };
    if hwnd.0.is_null() {
        bail!("Explorer shell window is unavailable");
    }

    let mut process_id = 0;
    // SAFETY: `process_id` is valid writable storage for the duration of the call.
    unsafe { GetWindowThreadProcessId(hwnd, Some(&raw mut process_id)) };
    if process_id == 0 {
        bail!("Explorer shell process identity is unavailable");
    }

    // SAFETY: the process ID came from the current shell window. The returned handle is owned.
    let process = OwnedHandle(
        unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id) }
            .context("open Explorer process")?,
    );
    let mut created = FILETIME::default();
    let mut exited = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    // SAFETY: all `FILETIME` pointers refer to valid writable storage and the process handle is
    // open for query access.
    unsafe {
        GetProcessTimes(
            process.0,
            &raw mut created,
            &raw mut exited,
            &raw mut kernel,
            &raw mut user,
        )
    }
    .context("read Explorer process creation time")?;

    Ok(ShellIdentity {
        process_id,
        created_100ns: u64::from(created.dwLowDateTime) | (u64::from(created.dwHighDateTime) << 32),
    })
}

/// Returns the primary monitor rectangle and current work area.
///
/// # Errors
///
/// Returns an error when Windows cannot provide monitor information.
pub fn primary_monitor() -> anyhow::Result<(Rect, Rect)> {
    // SAFETY: `MonitorFromPoint` has no pointer arguments. The default flag guarantees a monitor.
    let monitor = unsafe { MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY) };
    let mut info = MONITORINFO {
        cbSize: u32::try_from(size_of::<MONITORINFO>()).context("MONITORINFO size")?,
        ..Default::default()
    };
    // SAFETY: `info` is initialized with the required size and remains writable for the call.
    unsafe { GetMonitorInfoW(monitor, &raw mut info) }
        .ok()
        .context("read primary monitor")?;
    Ok((rect(info.rcMonitor), rect(info.rcWork)))
}

/// Returns a window's current physical rectangle.
///
/// # Errors
///
/// Returns an error when `hwnd` is not queryable.
pub fn window_rect(hwnd: HWND) -> anyhow::Result<Rect> {
    let mut value = RECT::default();
    // SAFETY: `value` is valid writable storage and `hwnd` is supplied by the caller.
    unsafe { GetWindowRect(hwnd, &raw mut value) }.context("read window rectangle")?;
    Ok(rect(value))
}

#[must_use]
pub fn is_window_visible(hwnd: HWND) -> bool {
    // SAFETY: `IsWindowVisible` only reads state associated with the supplied handle.
    unsafe { IsWindowVisible(hwnd) }.as_bool()
}

#[must_use]
pub fn rect(value: RECT) -> Rect {
    Rect {
        left: value.left,
        top: value.top,
        right: value.right,
        bottom: value.bottom,
    }
}

#[must_use]
pub fn raw_rect(value: Rect) -> RECT {
    RECT {
        left: value.left,
        top: value.top,
        right: value.right,
        bottom: value.bottom,
    }
}

/// Reads the PE optional-header subsystem without translating the native path.
///
/// # Errors
///
/// Returns an error when the image cannot be read or has an invalid PE shape.
pub fn pe_subsystem(path: &Path) -> anyhow::Result<u16> {
    let mut file = File::open(path).context("open PE image")?;
    let mut dos_header = [0_u8; 64];
    file.read_exact(&mut dos_header)
        .context("read DOS header")?;
    if dos_header.get(..2) != Some(b"MZ") {
        bail!("image has no MZ signature");
    }

    let pe_offset = u32::from_le_bytes(
        dos_header
            .get(60..64)
            .context("DOS header has no PE offset")?
            .try_into()
            .context("decode PE offset")?,
    );
    file.seek(SeekFrom::Start(u64::from(pe_offset)))
        .context("seek PE header")?;
    let mut signature = [0_u8; 4];
    file.read_exact(&mut signature)
        .context("read PE signature")?;
    if signature != *b"PE\0\0" {
        bail!("image has no PE signature");
    }

    file.seek(SeekFrom::Current(20 + 68))
        .context("seek PE subsystem")?;
    let mut subsystem = [0_u8; 2];
    file.read_exact(&mut subsystem)
        .context("read PE subsystem")?;
    Ok(u16::from_le_bytes(subsystem))
}

/// Ends the current Explorer generation and starts the system Explorer image.
///
/// # Errors
///
/// Returns an error when the process cannot be opened, ended, awaited, or restarted.
pub fn restart_explorer() -> anyhow::Result<u32> {
    const SHELL_EXIT_DEADLINE_MS: u32 = 10_000;

    let shell = shell_identity()?;
    // SAFETY: the PID is the current Explorer shell generation. The returned handle is uniquely
    // owned and requests only termination and native wait rights.
    let process = OwnedHandle(
        unsafe {
            OpenProcess(
                PROCESS_TERMINATE | PROCESS_SYNCHRONIZE,
                false,
                shell.process_id,
            )
        }
        .context("open Explorer for lifecycle restart")?,
    );
    // SAFETY: this disposable probe intentionally ends the current shell generation so Windows
    // broadcasts TaskbarCreated for the replacement generation.
    unsafe { TerminateProcess(process.0, 0) }.context("terminate Explorer")?;
    // SAFETY: the process handle has synchronization rights and remains live for this wait.
    let wait = unsafe { WaitForSingleObject(process.0, SHELL_EXIT_DEADLINE_MS) };
    if wait != WAIT_OBJECT_0 {
        bail!("Explorer did not exit before the native wait deadline: {wait:?}");
    }

    let windows_directory = std::env::var_os("WINDIR").context("WINDIR is unavailable")?;
    let explorer = Path::new(&windows_directory).join("explorer.exe");
    // `Command` receives the native `Path` directly. Do not preflight with a separate metadata
    // syscall: spawning is the authority and avoids a check/use race.
    Command::new(&explorer)
        .spawn()
        .context("start Explorer from the native WINDIR path")?;
    Ok(shell.process_id)
}
