use std::mem::size_of;
use std::num::NonZeroU32;

use windows::Win32::Foundation::LPARAM;
use windows::Win32::Foundation::WPARAM;
use windows::Win32::Graphics::Gdi::GetMonitorInfoW;
use windows::Win32::Graphics::Gdi::MONITOR_DEFAULTTONEAREST;
use windows::Win32::Graphics::Gdi::MONITORINFO;
use windows::Win32::Graphics::Gdi::MonitorFromWindow;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNOACTIVATE;
use windows::Win32::UI::WindowsAndMessaging::SWP_NOACTIVATE;
use windows::Win32::UI::WindowsAndMessaging::SWP_NOOWNERZORDER;
use windows::Win32::UI::WindowsAndMessaging::SWP_NOZORDER;
use windows::Win32::UI::WindowsAndMessaging::SetWindowPos;
use windows::Win32::UI::WindowsAndMessaging::ShowWindow;

use crate::AppBarEdge;
use crate::AppBarGeometry;
use crate::AppBarHostPlatform;
use crate::AppBarVisibility;
use crate::BorrowedAppBarWindow;
use crate::LogicalAppBarThickness;
use crate::PhysicalRect;
use crate::ShellGeneration;
use crate::WindowsAppBarApi;
use crate::WindowsAppBarError;
use crate::WindowsAppBarMessages;

pub struct WindowsAppBarPlatform {
    window: BorrowedAppBarWindow,
    messages: WindowsAppBarMessages,
    placement: WindowsAppBarPlacement,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindowsAppBarPlacement {
    edge: AppBarEdge,
    thickness: LogicalAppBarThickness,
}

impl WindowsAppBarPlacement {
    #[must_use]
    pub const fn new(edge: AppBarEdge, thickness: LogicalAppBarThickness) -> Self {
        Self { edge, thickness }
    }

    fn resolve(self, window: BorrowedAppBarWindow) -> Result<AppBarGeometry, WindowsAppBarError> {
        // SAFETY: the thread-affine window is live for this platform adapter.
        let monitor = unsafe { MonitorFromWindow(window.hwnd(), MONITOR_DEFAULTTONEAREST) };
        if monitor.0.is_null() {
            return Err(WindowsAppBarError::MonitorUnavailable);
        }
        let mut info = MONITORINFO {
            cbSize: u32::try_from(size_of::<MONITORINFO>())
                .map_err(|_| WindowsAppBarError::StructureSizeOverflow)?,
            ..Default::default()
        };
        // SAFETY: `info` has the required size and valid writable storage.
        unsafe { GetMonitorInfoW(monitor, &raw mut info).ok()? };
        // SAFETY: the thread-affine window is live for this platform adapter.
        let dpi = NonZeroU32::new(unsafe { GetDpiForWindow(window.hwnd()) })
            .ok_or(WindowsAppBarError::WindowDpiUnavailable)?;
        let monitor = PhysicalRect::new(
            info.rcMonitor.left,
            info.rcMonitor.top,
            info.rcMonitor.right,
            info.rcMonitor.bottom,
        )?;
        let thickness = self.thickness.to_physical(dpi)?;
        Ok(AppBarGeometry::new(monitor, self.edge, thickness))
    }
}

impl WindowsAppBarPlatform {
    #[must_use]
    pub const fn new(
        window: BorrowedAppBarWindow,
        messages: WindowsAppBarMessages,
        placement: WindowsAppBarPlacement,
    ) -> Self {
        Self {
            window,
            messages,
            placement,
        }
    }
}

impl AppBarHostPlatform for WindowsAppBarPlatform {
    type Error = WindowsAppBarError;
    type Geometry = WindowsAppBarPlacement;

    fn shell_generation(&mut self) -> Result<ShellGeneration, Self::Error> {
        WindowsAppBarApi::shell_generation()
    }

    fn register(&mut self) -> Result<(), Self::Error> {
        WindowsAppBarApi::register(self.window, self.messages.callback())
    }

    fn schedule_position(&mut self) -> Result<(), Self::Error> {
        // SAFETY: the borrowed window is live on this thread and the registered
        // message carries no pointer payload.
        unsafe {
            PostMessageW(
                Some(self.window.hwnd()),
                self.messages.position().id(),
                WPARAM(0),
                LPARAM(0),
            )?;
        }
        Ok(())
    }

    fn position(&mut self, visibility: AppBarVisibility) -> Result<(), Self::Error> {
        let geometry = self.placement.resolve(self.window)?;
        let rect = WindowsAppBarApi::reserve(self.window, geometry)?;
        let width = i32::try_from(rect.width().get())
            .map_err(|_| WindowsAppBarError::GeometrySpanOverflow)?;
        let height = i32::try_from(rect.height().get())
            .map_err(|_| WindowsAppBarError::GeometrySpanOverflow)?;
        // SAFETY: the borrowed window is live on this thread, the dimensions
        // are positive, and the flags preserve activation and Z-order.
        unsafe {
            SetWindowPos(
                self.window.hwnd(),
                None,
                rect.left(),
                rect.top(),
                width,
                height,
                SWP_NOACTIVATE | SWP_NOOWNERZORDER | SWP_NOZORDER,
            )?;
        }
        if visibility == AppBarVisibility::RevealAfterPosition {
            // SAFETY: the window remains live and this command cannot activate it.
            let _ = unsafe { ShowWindow(self.window.hwnd(), SW_SHOWNOACTIVATE) };
        }
        Ok(())
    }

    fn remove(&mut self) -> Result<(), Self::Error> {
        WindowsAppBarApi::remove(self.window)
    }

    fn update_geometry(&mut self, placement: WindowsAppBarPlacement) {
        self.placement = placement;
    }
}
