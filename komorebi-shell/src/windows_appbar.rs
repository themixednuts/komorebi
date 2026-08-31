use std::ffi::c_void;
use std::mem::size_of;
use std::num::NonZeroIsize;
use std::num::NonZeroU32;
use std::num::NonZeroU64;

use thiserror::Error;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::Foundation::FILETIME;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Foundation::HWND;
use windows::Win32::Foundation::RECT;
use windows::Win32::System::Threading::GetProcessTimes;
use windows::Win32::System::Threading::OpenProcess;
use windows::Win32::System::Threading::PROCESS_QUERY_LIMITED_INFORMATION;
use windows::Win32::UI::Shell::ABE_BOTTOM;
use windows::Win32::UI::Shell::ABE_LEFT;
use windows::Win32::UI::Shell::ABE_RIGHT;
use windows::Win32::UI::Shell::ABE_TOP;
use windows::Win32::UI::Shell::ABM_NEW;
use windows::Win32::UI::Shell::ABM_QUERYPOS;
use windows::Win32::UI::Shell::ABM_REMOVE;
use windows::Win32::UI::Shell::ABM_SETPOS;
use windows::Win32::UI::Shell::APPBARDATA;
use windows::Win32::UI::Shell::SHAppBarMessage;
use windows::Win32::UI::WindowsAndMessaging::GetShellWindow;
use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
use windows::Win32::UI::WindowsAndMessaging::WM_APP;

use crate::AppBarEdge;
use crate::AppBarGeometry;
use crate::PhysicalRect;
use crate::PhysicalRectError;
use crate::ShellGeneration;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AppBarCallbackMessage(u32);

impl AppBarCallbackMessage {
    #[must_use]
    pub const fn new(message: u32) -> Option<Self> {
        if message >= WM_APP && message < 0xc000 {
            Some(Self(message))
        } else {
            None
        }
    }

    const fn get(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BorrowedAppBarWindow(HWND);

impl BorrowedAppBarWindow {
    /// Borrows a live UI-thread-owned Win32 window for an `AppBar` call.
    ///
    /// # Safety
    ///
    /// `raw` must identify a live top-level window and all methods using this
    /// value must run on that window's owning thread.
    pub unsafe fn from_raw(raw: NonZeroIsize) -> Self {
        Self(HWND(raw.get() as *mut c_void))
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct WindowsAppBarApi;

impl WindowsAppBarApi {
    pub fn shell_generation() -> Result<ShellGeneration, WindowsAppBarError> {
        // SAFETY: `GetShellWindow` has no preconditions.
        let shell = unsafe { GetShellWindow() };
        if shell.0.is_null() {
            return Err(WindowsAppBarError::ShellWindowUnavailable);
        }
        let mut process_id = 0;
        // SAFETY: the process ID output points to valid writable storage.
        unsafe { GetWindowThreadProcessId(shell, Some(&raw mut process_id)) };
        let process_id =
            NonZeroU32::new(process_id).ok_or(WindowsAppBarError::ShellProcessUnavailable)?;
        // SAFETY: the PID came from the current shell window and the returned
        // handle is uniquely owned by `OwnedProcess`.
        let process = OwnedProcess(unsafe {
            OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id.get())?
        });
        let mut created = FILETIME::default();
        let mut exited = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        // SAFETY: every FILETIME output is valid and the process handle has query access.
        unsafe {
            GetProcessTimes(
                process.0,
                &raw mut created,
                &raw mut exited,
                &raw mut kernel,
                &raw mut user,
            )?;
        };
        let created = u64::from(created.dwLowDateTime) | (u64::from(created.dwHighDateTime) << 32);
        let created_100ns =
            NonZeroU64::new(created).ok_or(WindowsAppBarError::ShellCreationTimeUnavailable)?;
        Ok(ShellGeneration::new(process_id, created_100ns))
    }

    pub fn register(
        window: BorrowedAppBarWindow,
        callback: AppBarCallbackMessage,
    ) -> Result<(), WindowsAppBarError> {
        let mut data = appbar_data(window)?;
        data.uCallbackMessage = callback.get();
        appbar_call(ABM_NEW, &mut data, "ABM_NEW")
    }

    pub fn remove(window: BorrowedAppBarWindow) -> Result<(), WindowsAppBarError> {
        let mut data = appbar_data(window)?;
        appbar_call(ABM_REMOVE, &mut data, "ABM_REMOVE")
    }

    pub fn reserve(
        window: BorrowedAppBarWindow,
        geometry: AppBarGeometry,
    ) -> Result<PhysicalRect, WindowsAppBarError> {
        let proposed = geometry.proposed_rect()?;
        let mut data = appbar_data(window)?;
        data.uEdge = edge_code(geometry);
        data.rc = raw_rect(proposed);
        appbar_call(ABM_QUERYPOS, &mut data, "ABM_QUERYPOS")?;
        let negotiated = geometry.apply_thickness(physical_rect(data.rc)?)?;
        data.rc = raw_rect(negotiated);
        appbar_call(ABM_SETPOS, &mut data, "ABM_SETPOS")?;
        physical_rect(data.rc).map_err(WindowsAppBarError::Geometry)
    }
}

fn edge_code(geometry: AppBarGeometry) -> u32 {
    match geometry.edge() {
        AppBarEdge::Left => ABE_LEFT,
        AppBarEdge::Top => ABE_TOP,
        AppBarEdge::Right => ABE_RIGHT,
        AppBarEdge::Bottom => ABE_BOTTOM,
    }
}

fn appbar_data(window: BorrowedAppBarWindow) -> Result<APPBARDATA, WindowsAppBarError> {
    Ok(APPBARDATA {
        cbSize: u32::try_from(size_of::<APPBARDATA>())
            .map_err(|_| WindowsAppBarError::StructureSizeOverflow)?,
        hWnd: window.0,
        ..Default::default()
    })
}

fn appbar_call(
    message: u32,
    data: &mut APPBARDATA,
    operation: &'static str,
) -> Result<(), WindowsAppBarError> {
    // SAFETY: `data` is initialized for its operation and remains exclusively borrowed.
    if unsafe { SHAppBarMessage(message, data) } == 0 {
        Err(WindowsAppBarError::NativeCallFailed(operation))
    } else {
        Ok(())
    }
}

fn raw_rect(rect: PhysicalRect) -> RECT {
    RECT {
        left: rect.left(),
        top: rect.top(),
        right: rect.right(),
        bottom: rect.bottom(),
    }
}

fn physical_rect(rect: RECT) -> Result<PhysicalRect, PhysicalRectError> {
    PhysicalRect::new(rect.left, rect.top, rect.right, rect.bottom)
}

struct OwnedProcess(HANDLE);

impl Drop for OwnedProcess {
    fn drop(&mut self) {
        // SAFETY: this wrapper owns exactly one successful `OpenProcess` handle.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

#[derive(Debug, Error)]
pub enum WindowsAppBarError {
    #[error("the Windows shell window is unavailable")]
    ShellWindowUnavailable,
    #[error("the Windows shell process is unavailable")]
    ShellProcessUnavailable,
    #[error("the Windows shell creation time is unavailable")]
    ShellCreationTimeUnavailable,
    #[error("{0} failed")]
    NativeCallFailed(&'static str),
    #[error("APPBARDATA size does not fit the Windows ABI field")]
    StructureSizeOverflow,
    #[error(transparent)]
    Geometry(#[from] PhysicalRectError),
    #[error(transparent)]
    Windows(#[from] windows::core::Error),
}
