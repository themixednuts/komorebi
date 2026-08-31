use std::ffi::c_void;
use std::marker::PhantomData;
use std::mem::size_of;
use std::num::NonZeroIsize;
use std::num::NonZeroU32;
use std::num::NonZeroU64;
use std::rc::Rc;

use thiserror::Error;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::Foundation::FILETIME;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Foundation::HWND;
use windows::Win32::Foundation::RECT;
use windows::Win32::Foundation::WPARAM;
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
use windows::Win32::UI::Shell::ABN_POSCHANGED;
use windows::Win32::UI::Shell::APPBARDATA;
use windows::Win32::UI::Shell::SHAppBarMessage;
use windows::Win32::UI::WindowsAndMessaging::GetShellWindow;
use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
use windows::Win32::UI::WindowsAndMessaging::RegisterWindowMessageW;
use windows::Win32::UI::WindowsAndMessaging::WM_DISPLAYCHANGE;
use windows::Win32::UI::WindowsAndMessaging::WM_DPICHANGED;
use windows::Win32::UI::WindowsAndMessaging::WM_NCDESTROY;
use windows::core::PCWSTR;
use windows::core::w;

use crate::AppBarEdge;
use crate::AppBarGeometry;
use crate::LogicalAppBarThicknessError;
use crate::PhysicalRect;
use crate::PhysicalRectError;
use crate::ShellGeneration;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AppBarCallbackMessage(NonZeroU32);

impl AppBarCallbackMessage {
    #[must_use]
    pub const fn id(self) -> u32 {
        self.0.get()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AppBarPositionMessage(NonZeroU32);

impl AppBarPositionMessage {
    #[must_use]
    pub const fn id(self) -> u32 {
        self.0.get()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TaskbarCreatedMessage(NonZeroU32);

impl TaskbarCreatedMessage {
    #[must_use]
    pub const fn id(self) -> u32 {
        self.0.get()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WindowsAppBarMessages {
    callback: AppBarCallbackMessage,
    position: AppBarPositionMessage,
    taskbar_created: TaskbarCreatedMessage,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindowsAppBarSignal {
    PositionInvalidated,
    PositionRequested,
    ShellRecreated,
    Destroying,
    Forward,
}

impl WindowsAppBarMessages {
    pub fn register() -> Result<Self, WindowsAppBarError> {
        Ok(Self {
            callback: AppBarCallbackMessage(register_message(
                w!("komorebi.shell.appbar.callback.v1"),
                "appbar callback",
            )?),
            position: AppBarPositionMessage(register_message(
                w!("komorebi.shell.appbar.position.v1"),
                "appbar position",
            )?),
            taskbar_created: TaskbarCreatedMessage(register_message(
                w!("TaskbarCreated"),
                "TaskbarCreated",
            )?),
        })
    }

    #[must_use]
    pub const fn callback(self) -> AppBarCallbackMessage {
        self.callback
    }

    #[must_use]
    pub const fn position(self) -> AppBarPositionMessage {
        self.position
    }

    #[must_use]
    pub const fn taskbar_created(self) -> TaskbarCreatedMessage {
        self.taskbar_created
    }

    #[must_use]
    pub const fn classify(self, message: u32, wparam: WPARAM) -> WindowsAppBarSignal {
        if (message == self.callback.id() && wparam.0 == ABN_POSCHANGED as usize)
            || message == WM_DISPLAYCHANGE
            || message == WM_DPICHANGED
        {
            WindowsAppBarSignal::PositionInvalidated
        } else if message == self.position.id() {
            WindowsAppBarSignal::PositionRequested
        } else if message == self.taskbar_created.id() {
            WindowsAppBarSignal::ShellRecreated
        } else if message == WM_NCDESTROY {
            WindowsAppBarSignal::Destroying
        } else {
            WindowsAppBarSignal::Forward
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BorrowedAppBarWindow {
    hwnd: HWND,
    _thread_affine: PhantomData<Rc<()>>,
}

impl BorrowedAppBarWindow {
    /// Borrows a live UI-thread-owned Win32 window for an `AppBar` call.
    ///
    /// # Safety
    ///
    /// `raw` must identify a live top-level window and all methods using this
    /// value must run on that window's owning thread.
    pub unsafe fn from_raw(raw: NonZeroIsize) -> Self {
        Self {
            hwnd: HWND(raw.get() as *mut c_void),
            _thread_affine: PhantomData,
        }
    }

    pub(crate) const fn hwnd(self) -> HWND {
        self.hwnd
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
        data.uCallbackMessage = callback.id();
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

fn register_message(name: PCWSTR, label: &'static str) -> Result<NonZeroU32, WindowsAppBarError> {
    // SAFETY: each pointer comes from a static, NUL-terminated `w!` string.
    NonZeroU32::new(unsafe { RegisterWindowMessageW(name) })
        .ok_or(WindowsAppBarError::WindowMessageRegistrationFailed(label))
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
        hWnd: window.hwnd(),
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
    #[error("could not register the {0} window message")]
    WindowMessageRegistrationFailed(&'static str),
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
    #[error("AppBar geometry span does not fit the Windows positioning API")]
    GeometrySpanOverflow,
    #[error("the AppBar window is not associated with a monitor")]
    MonitorUnavailable,
    #[error("the AppBar window DPI is unavailable")]
    WindowDpiUnavailable,
    #[error(transparent)]
    Geometry(#[from] PhysicalRectError),
    #[error(transparent)]
    LogicalThickness(#[from] LogicalAppBarThicknessError),
    #[error(transparent)]
    Windows(#[from] windows::core::Error),
}
