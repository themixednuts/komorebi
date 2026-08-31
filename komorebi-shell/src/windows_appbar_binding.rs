use std::cell::Cell;
use std::cell::RefCell;
use std::panic::AssertUnwindSafe;
use std::panic::catch_unwind;

use thiserror::Error;
use windows::Win32::Foundation::HWND;
use windows::Win32::Foundation::LPARAM;
use windows::Win32::Foundation::LRESULT;
use windows::Win32::Foundation::WPARAM;
use windows::Win32::UI::Shell::DefSubclassProc;
use windows::Win32::UI::Shell::RemoveWindowSubclass;
use windows::Win32::UI::Shell::SetWindowSubclass;
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

use crate::AppBarGeometry;
use crate::AppBarHost;
use crate::AppBarHostError;
use crate::BorrowedAppBarWindow;
use crate::WindowsAppBarError;
use crate::WindowsAppBarMessages;
use crate::WindowsAppBarPlatform;
use crate::WindowsAppBarSignal;

type NativeHost = AppBarHost<WindowsAppBarPlatform>;
type NativeHostError = AppBarHostError<WindowsAppBarError>;

pub struct WindowsAppBarBinding {
    state: Option<Box<BindingState>>,
}

impl WindowsAppBarBinding {
    pub fn install(
        window: BorrowedAppBarWindow,
        geometry: AppBarGeometry,
        report: impl Fn(WindowsAppBarRuntimeError) + 'static,
    ) -> Result<Self, WindowsAppBarInstallError> {
        let messages = WindowsAppBarMessages::register()?;
        let platform = WindowsAppBarPlatform::new(window, messages, geometry);
        let mut state = Box::new(BindingState {
            host: RefCell::new(AppBarHost::new(platform)),
            window,
            messages,
            native_position_invalidated: Cell::new(false),
            shell_recreated: Cell::new(false),
            wake_queued: Cell::new(false),
            installed: Cell::new(false),
            window_destroyed: Cell::new(false),
            report: Box::new(report),
        });

        if let Err(start) = state.host.get_mut().start() {
            return match state.host.get_mut().shutdown() {
                Ok(()) => Err(WindowsAppBarInstallError::Start(start)),
                Err(cleanup) => Err(WindowsAppBarInstallError::StartAndCleanup { start, cleanup }),
            };
        }

        let state_address = (&raw const *state) as usize;
        // SAFETY: `state` is heap allocated and cannot move while the subclass
        // is installed. Its address is both the subclass identity and callback data.
        let installed = unsafe {
            SetWindowSubclass(
                window.hwnd(),
                Some(appbar_subclass_proc),
                state_address,
                state_address,
            )
        }
        .as_bool();
        if !installed {
            return match state.host.get_mut().shutdown() {
                Ok(()) => Err(WindowsAppBarInstallError::SubclassInstallationFailed),
                Err(cleanup) => {
                    Err(WindowsAppBarInstallError::SubclassInstallationAndCleanup { cleanup })
                }
            };
        }
        state.installed.set(true);
        Ok(Self { state: Some(state) })
    }

    pub fn geometry_changed(
        &self,
        geometry: AppBarGeometry,
    ) -> Result<(), WindowsAppBarRuntimeError> {
        let state = self.state()?;
        state
            .host
            .borrow_mut()
            .geometry_changed(geometry)
            .map_err(WindowsAppBarRuntimeError::Host)
    }

    pub fn shutdown(&mut self) -> Result<(), WindowsAppBarRuntimeError> {
        let state = self.state()?;
        state
            .host
            .borrow_mut()
            .shutdown()
            .map_err(WindowsAppBarRuntimeError::Host)?;
        if state.installed.replace(false) && !state.window_destroyed.get() {
            // SAFETY: this exact callback and pointer-derived identity were
            // installed on the same live UI-thread-owned window.
            if !unsafe {
                RemoveWindowSubclass(
                    state.window().hwnd(),
                    Some(appbar_subclass_proc),
                    state.subclass_id(),
                )
            }
            .as_bool()
            {
                state.installed.set(true);
                return Err(WindowsAppBarRuntimeError::SubclassRemovalFailed);
            }
        }
        Ok(())
    }

    fn state(&self) -> Result<&BindingState, WindowsAppBarRuntimeError> {
        self.state
            .as_deref()
            .ok_or(WindowsAppBarRuntimeError::BindingReleased)
    }
}

impl Drop for WindowsAppBarBinding {
    fn drop(&mut self) {
        let Some(state) = self.state.take() else {
            return;
        };
        if let Err(error) = state.host.borrow_mut().shutdown() {
            state.report(WindowsAppBarRuntimeError::Host(error));
        }
        if state.installed.get() && !state.window_destroyed.get() {
            // SAFETY: this exact subclass registration still owns `state`.
            let removed = unsafe {
                RemoveWindowSubclass(
                    state.window().hwnd(),
                    Some(appbar_subclass_proc),
                    state.subclass_id(),
                )
            }
            .as_bool();
            if !removed {
                state.report(WindowsAppBarRuntimeError::SubclassRemovalFailed);
                let _ = Box::leak(state);
            }
        }
    }
}

struct BindingState {
    host: RefCell<NativeHost>,
    window: BorrowedAppBarWindow,
    messages: WindowsAppBarMessages,
    native_position_invalidated: Cell<bool>,
    shell_recreated: Cell<bool>,
    wake_queued: Cell<bool>,
    installed: Cell<bool>,
    window_destroyed: Cell<bool>,
    report: Box<dyn Fn(WindowsAppBarRuntimeError)>,
}

impl BindingState {
    fn dispatch(&self, message: u32, wparam: WPARAM) {
        match self.messages.classify(message, wparam) {
            WindowsAppBarSignal::PositionInvalidated => {
                self.native_position_invalidated.set(true);
                self.queue_wake();
            }
            WindowsAppBarSignal::ShellRecreated => {
                self.shell_recreated.set(true);
                self.queue_wake();
            }
            WindowsAppBarSignal::PositionRequested => self.handle_wake(),
            WindowsAppBarSignal::Destroying => self.handle_destroying(),
            WindowsAppBarSignal::Forward => {}
        }
    }

    fn queue_wake(&self) {
        if self.wake_queued.replace(true) {
            return;
        }
        // SAFETY: the window is live while its subclass receives messages and
        // the registered wake message contains no pointer payload.
        let posted = unsafe {
            PostMessageW(
                Some(self.window().hwnd()),
                self.messages.position().id(),
                WPARAM(0),
                LPARAM(0),
            )
        };
        if let Err(error) = posted {
            self.wake_queued.set(false);
            self.report(WindowsAppBarRuntimeError::WakeQueue(error));
        }
    }

    fn handle_wake(&self) {
        self.wake_queued.set(false);
        let shell_recreated = self.shell_recreated.replace(false);
        let invalidated = self.native_position_invalidated.replace(false);
        let Ok(mut host) = self.host.try_borrow_mut() else {
            self.shell_recreated.set(shell_recreated);
            self.native_position_invalidated.set(invalidated);
            self.queue_wake();
            return;
        };
        if shell_recreated && let Err(error) = host.shell_recreated() {
            self.report(WindowsAppBarRuntimeError::Host(error));
        }
        let positioned = if invalidated {
            host.position_event_received()
        } else {
            host.position_requested()
        };
        if let Err(error) = positioned {
            self.report(WindowsAppBarRuntimeError::Host(error));
        }
    }

    fn handle_destroying(&self) {
        self.window_destroyed.set(true);
        if let Ok(mut host) = self.host.try_borrow_mut()
            && let Err(error) = host.shutdown()
        {
            self.report(WindowsAppBarRuntimeError::Host(error));
        }
        // SAFETY: WM_NCDESTROY is delivered on the owning thread for this exact
        // subclass registration. The boxed state outlives this callback.
        let removed = unsafe {
            RemoveWindowSubclass(
                self.window().hwnd(),
                Some(appbar_subclass_proc),
                self.subclass_id(),
            )
        }
        .as_bool();
        self.installed.set(!removed);
        if !removed {
            self.report(WindowsAppBarRuntimeError::SubclassRemovalFailed);
        }
    }

    fn window(&self) -> BorrowedAppBarWindow {
        self.window
    }

    fn subclass_id(&self) -> usize {
        std::ptr::from_ref(self) as usize
    }

    fn report(&self, error: WindowsAppBarRuntimeError) {
        let _ = catch_unwind(AssertUnwindSafe(|| (self.report)(error)));
    }
}

unsafe extern "system" fn appbar_subclass_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _subclass_id: usize,
    reference_data: usize,
) -> LRESULT {
    // SAFETY: installation stores the stable `BindingState` address as
    // reference data and failed removal leaks it rather than dangling it.
    let state = unsafe { &*(reference_data as *const BindingState) };
    if catch_unwind(AssertUnwindSafe(|| state.dispatch(message, wparam))).is_err() {
        state.report(WindowsAppBarRuntimeError::CallbackPanicked);
    }
    // SAFETY: every message continues through the comctl32 subclass chain.
    unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
}

#[derive(Debug, Error)]
pub enum WindowsAppBarInstallError {
    #[error(transparent)]
    Messages(#[from] WindowsAppBarError),
    #[error("could not start the AppBar host")]
    Start(#[source] NativeHostError),
    #[error("could not start or clean up the AppBar host")]
    StartAndCleanup {
        #[source]
        start: NativeHostError,
        cleanup: NativeHostError,
    },
    #[error("could not install the AppBar window subclass")]
    SubclassInstallationFailed,
    #[error("could not install the AppBar subclass or clean up its registration")]
    SubclassInstallationAndCleanup {
        #[source]
        cleanup: NativeHostError,
    },
}

#[derive(Debug, Error)]
pub enum WindowsAppBarRuntimeError {
    #[error("the AppBar binding has already released its native state")]
    BindingReleased,
    #[error(transparent)]
    Host(#[from] NativeHostError),
    #[error("could not queue the native AppBar wake message")]
    WakeQueue(#[source] windows::core::Error),
    #[error("could not remove the AppBar window subclass")]
    SubclassRemovalFailed,
    #[error("the AppBar subclass callback panicked")]
    CallbackPanicked,
}
