use windows::Win32::Foundation::LPARAM;
use windows::Win32::Foundation::WPARAM;
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNOACTIVATE;
use windows::Win32::UI::WindowsAndMessaging::SWP_NOACTIVATE;
use windows::Win32::UI::WindowsAndMessaging::SWP_NOOWNERZORDER;
use windows::Win32::UI::WindowsAndMessaging::SWP_NOZORDER;
use windows::Win32::UI::WindowsAndMessaging::SetWindowPos;
use windows::Win32::UI::WindowsAndMessaging::ShowWindow;

use crate::AppBarGeometry;
use crate::AppBarHostPlatform;
use crate::AppBarVisibility;
use crate::BorrowedAppBarWindow;
use crate::ShellGeneration;
use crate::WindowsAppBarApi;
use crate::WindowsAppBarError;
use crate::WindowsAppBarMessages;

pub struct WindowsAppBarPlatform {
    window: BorrowedAppBarWindow,
    messages: WindowsAppBarMessages,
    geometry: AppBarGeometry,
}

impl WindowsAppBarPlatform {
    #[must_use]
    pub const fn new(
        window: BorrowedAppBarWindow,
        messages: WindowsAppBarMessages,
        geometry: AppBarGeometry,
    ) -> Self {
        Self {
            window,
            messages,
            geometry,
        }
    }
}

impl AppBarHostPlatform for WindowsAppBarPlatform {
    type Error = WindowsAppBarError;

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
        let rect = WindowsAppBarApi::reserve(self.window, self.geometry)?;
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

    fn update_geometry(&mut self, geometry: AppBarGeometry) {
        self.geometry = geometry;
    }
}
