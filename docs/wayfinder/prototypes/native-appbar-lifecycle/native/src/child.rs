use std::cell::{Cell, RefCell};
use std::io::{BufRead, BufReader};
use std::mem::size_of;
use std::num::NonZeroU32;

use anyhow::{Context, bail};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Shell::{
    ABE_BOTTOM, ABE_LEFT, ABE_RIGHT, ABE_TOP, ABM_ACTIVATE, ABM_GETSTATE, ABM_NEW, ABM_QUERYPOS,
    ABM_REMOVE, ABM_SETPOS, ABM_WINDOWPOSCHANGED, ABN_FULLSCREENAPP, ABN_POSCHANGED,
    ABN_STATECHANGE, ABN_WINDOWARRANGE, ABS_ALWAYSONTOP, APPBARDATA, SHAppBarMessage,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CREATESTRUCTW, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GWLP_USERDATA,
    GetMessageW, GetWindowLongPtrW, HWND_BOTTOM, HWND_TOPMOST, MSG, MoveWindow, PostMessageW,
    PostQuitMessage, RegisterClassW, RegisterWindowMessageW, SW_HIDE, SW_SHOWNOACTIVATE,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetWindowLongPtrW, SetWindowPos, ShowWindow,
    TranslateMessage, WM_ACTIVATE, WM_APP, WM_CLOSE, WM_DESTROY, WM_DISPLAYCHANGE, WM_DPICHANGED,
    WM_NCCREATE, WM_NCDESTROY, WM_WINDOWPOSCHANGED, WNDCLASSW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_POPUP,
};
use windows::core::{PCWSTR, w};

use crate::model::{AppBarGeometry, Edge, Lifecycle, RegisterDecision};
use crate::protocol::{ChildCommand, ChildEvent, NotificationKind, PositionReason, write_event};
use crate::windows::{
    is_window_visible, primary_monitor, raw_rect, rect, shell_identity, window_rect,
};

const POSITION_MESSAGE: u32 = WM_APP + 1;
const SET_THICKNESS_MESSAGE: u32 = WM_APP + 2;
const SIMULATE_DPI_MESSAGE: u32 = WM_APP + 3;
const REGISTER_AGAIN_MESSAGE: u32 = WM_APP + 4;
const SHUTDOWN_MESSAGE: u32 = WM_APP + 5;
const DEFAULT_DPI: u32 = 96;
const WINDOW_CLASS: PCWSTR = w!("KomorebiNativeAppBarLifecyclePrototype");

#[derive(Clone, Copy)]
pub struct ChildOptions {
    pub edge: Edge,
    pub thickness_dip: NonZeroU32,
}

struct Host {
    hwnd: Cell<HWND>,
    callback_message: u32,
    taskbar_created_message: u32,
    lifecycle: RefCell<Lifecycle>,
    geometry: Cell<AppBarGeometry>,
    thickness_dip: Cell<NonZeroU32>,
    dpi: Cell<NonZeroU32>,
    position_reason: Cell<Option<PositionReason>>,
    shown: Cell<bool>,
}

impl Host {
    fn new(options: ChildOptions) -> anyhow::Result<Self> {
        // SAFETY: registered-message allocation has no pointer preconditions.
        let callback_message =
            unsafe { RegisterWindowMessageW(w!("Komorebi AppBar lifecycle prototype callback")) };
        // SAFETY: registered-message allocation has no pointer preconditions.
        let taskbar_created_message = unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) };
        if callback_message == 0 || taskbar_created_message == 0 {
            bail!("allocate AppBar window messages");
        }

        let (monitor, _) = primary_monitor()?;
        Ok(Self {
            hwnd: Cell::new(HWND::default()),
            callback_message,
            taskbar_created_message,
            lifecycle: RefCell::new(Lifecycle::default()),
            geometry: Cell::new(AppBarGeometry {
                monitor,
                edge: options.edge,
                thickness: i32::try_from(options.thickness_dip.get())
                    .context("initial AppBar thickness")?,
            }),
            thickness_dip: Cell::new(options.thickness_dip),
            dpi: Cell::new(NonZeroU32::new(DEFAULT_DPI).context("default DPI is zero")?),
            position_reason: Cell::new(None),
            shown: Cell::new(false),
        })
    }

    fn emit(event: &ChildEvent) -> anyhow::Result<()> {
        write_event(std::io::stdout().lock(), event)
    }

    fn appbar_data(&self) -> anyhow::Result<APPBARDATA> {
        Ok(APPBARDATA {
            cbSize: u32::try_from(size_of::<APPBARDATA>()).context("APPBARDATA size")?,
            hWnd: self.hwnd.get(),
            ..Default::default()
        })
    }

    fn register(&self, reason: PositionReason) -> anyhow::Result<()> {
        let shell = shell_identity()?;
        match self.lifecycle.borrow_mut().begin_registration(shell) {
            RegisterDecision::AlreadyRegistered => {
                Self::emit(&ChildEvent::RegistrationSuppressed { shell })?;
                return Ok(());
            }
            RegisterDecision::Destroyed => bail!("register destroyed AppBar"),
            RegisterDecision::Register => {}
        }

        let mut data = self.appbar_data()?;
        data.uCallbackMessage = self.callback_message;
        // SAFETY: `data` has the required size, live AppBar HWND, and callback message.
        if unsafe { SHAppBarMessage(ABM_NEW, &raw mut data) } == 0 {
            self.lifecycle.borrow_mut().registration_failed(shell);
            bail!("Windows rejected AppBar registration");
        }

        self.lifecycle.borrow_mut().registration_succeeded(shell);
        Self::emit(&ChildEvent::Registered { shell })?;
        self.queue_position(reason)
    }

    fn queue_position(&self, reason: PositionReason) -> anyhow::Result<()> {
        self.position_reason
            .set(Some(match self.position_reason.get() {
                Some(current) => current.merge(reason),
                None => reason,
            }));
        if self.lifecycle.borrow_mut().request_position() {
            // SAFETY: the target is this thread's live HWND and the message carries no pointers.
            unsafe {
                PostMessageW(
                    Some(self.hwnd.get()),
                    POSITION_MESSAGE,
                    WPARAM(0),
                    LPARAM(0),
                )
            }
            .context("queue AppBar position")?;
        }
        Ok(())
    }

    fn position(&self) -> anyhow::Result<()> {
        if !self.lifecycle.borrow_mut().begin_position() {
            return Ok(());
        }

        let reason = self
            .position_reason
            .take()
            .context("queued AppBar position has no cause")?;
        let result = self.position_inner(reason);
        let queue_another = self.lifecycle.borrow_mut().finish_position();
        if queue_another {
            // SAFETY: the target is this thread's live HWND and the message carries no pointers.
            unsafe {
                PostMessageW(
                    Some(self.hwnd.get()),
                    POSITION_MESSAGE,
                    WPARAM(0),
                    LPARAM(0),
                )
            }
            .context("queue invalidated AppBar position")?;
        }
        result
    }

    fn position_inner(&self, reason: PositionReason) -> anyhow::Result<()> {
        let geometry = self.geometry.get();
        let mut data = self.appbar_data()?;
        data.uEdge = edge_value(geometry.edge);
        data.rc = raw_rect(geometry.proposed_rect());

        // SAFETY: `data` contains the registered HWND, requested edge, and writable rectangle.
        unsafe { SHAppBarMessage(ABM_QUERYPOS, &raw mut data) };
        data.rc = raw_rect(geometry.apply_thickness(rect(data.rc)));
        // SAFETY: `data` contains the registered HWND and the rectangle approved by the query.
        unsafe { SHAppBarMessage(ABM_SETPOS, &raw mut data) };
        let negotiated = rect(data.rc);
        // SAFETY: the negotiated dimensions come from the Shell and the HWND belongs to this host.
        unsafe {
            MoveWindow(
                self.hwnd.get(),
                negotiated.left,
                negotiated.top,
                negotiated.width(),
                negotiated.height(),
                true,
            )
        }
        .context("move AppBar to negotiated rectangle")?;

        let (_, work_area) = primary_monitor()?;
        Self::emit(&ChildEvent::Positioned {
            reason,
            rect: negotiated,
            work_area,
        })?;

        if !self.shown.replace(true) {
            // SAFETY: the HWND has already been moved to the Shell-negotiated rectangle.
            let _was_visible = unsafe { ShowWindow(self.hwnd.get(), SW_SHOWNOACTIVATE) };
            Self::emit(&ChildEvent::Shown {
                rect: window_rect(self.hwnd.get())?,
                work_area,
                visible_before_position: false,
            })?;
        }
        Ok(())
    }

    fn update_thickness(
        &self,
        thickness_dip: NonZeroU32,
        reason: PositionReason,
    ) -> anyhow::Result<()> {
        self.thickness_dip.set(thickness_dip);
        let physical = physical_thickness(self.thickness_dip.get(), self.dpi.get())?;
        self.geometry.set(AppBarGeometry {
            thickness: physical,
            ..self.geometry.get()
        });
        self.queue_position(reason)
    }

    fn update_monitor(&self) -> anyhow::Result<()> {
        let (monitor, _) = primary_monitor()?;
        self.geometry.set(AppBarGeometry {
            monitor,
            ..self.geometry.get()
        });
        self.queue_position(PositionReason::GeometryChanged)
    }

    fn update_dpi(&self, dpi: NonZeroU32) -> anyhow::Result<()> {
        self.dpi.set(dpi);
        self.update_thickness(self.thickness_dip.get(), PositionReason::GeometryChanged)
    }

    fn release(&self) -> anyhow::Result<()> {
        if self.lifecycle.borrow_mut().detach() {
            let mut data = self.appbar_data()?;
            // SAFETY: `data` identifies the currently registered HWND. ABM_REMOVE ignores the
            // remaining fields and always returns true by contract.
            unsafe { SHAppBarMessage(ABM_REMOVE, &raw mut data) };
        }
        Self::emit(&ChildEvent::Released)
    }

    fn destroy(&self) -> anyhow::Result<()> {
        if self.lifecycle.borrow_mut().destroy() {
            let mut data = self.appbar_data()?;
            // SAFETY: destruction is the last-resort cleanup path for a still-registered HWND.
            // The Shell contract makes duplicate ABM_REMOVE harmless.
            unsafe { SHAppBarMessage(ABM_REMOVE, &raw mut data) };
        }
        Ok(())
    }

    fn notify_shell(&self, message: u32) -> anyhow::Result<()> {
        let mut data = self.appbar_data()?;
        // SAFETY: the message is one of the AppBar notification acknowledgements and data contains
        // the current HWND.
        unsafe { SHAppBarMessage(message, &raw mut data) };
        Ok(())
    }

    fn update_z_order(&self, fullscreen_open: bool) -> anyhow::Result<()> {
        let insert_after = if fullscreen_open {
            HWND_BOTTOM
        } else {
            let mut data = self.appbar_data()?;
            // SAFETY: ABM_GETSTATE reads taskbar state using a valid APPBARDATA value.
            let state = u32::try_from(unsafe { SHAppBarMessage(ABM_GETSTATE, &raw mut data) })
                .context("read AppBar state")?;
            if state & ABS_ALWAYSONTOP != 0 {
                HWND_TOPMOST
            } else {
                HWND_BOTTOM
            }
        };
        // SAFETY: the call changes only z-order and explicitly forbids move, resize, or activation.
        unsafe {
            SetWindowPos(
                self.hwnd.get(),
                Some(insert_after),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            )
        }
        .context("update AppBar z-order")
    }

    fn appbar_notification(&self, code: u32, value: LPARAM) -> anyhow::Result<()> {
        match code {
            ABN_POSCHANGED => {
                Self::emit(&ChildEvent::Notification {
                    notification: NotificationKind::PositionChanged,
                })?;
                self.queue_position(PositionReason::ShellPositionChanged)
            }
            ABN_FULLSCREENAPP => {
                let opened = value.0 != 0;
                Self::emit(&ChildEvent::Notification {
                    notification: if opened {
                        NotificationKind::FullscreenOpened
                    } else {
                        NotificationKind::FullscreenClosed
                    },
                })?;
                self.update_z_order(opened)
            }
            ABN_STATECHANGE => {
                Self::emit(&ChildEvent::Notification {
                    notification: NotificationKind::StateChanged,
                })?;
                self.update_z_order(false)
            }
            ABN_WINDOWARRANGE => {
                let started = value.0 != 0;
                Self::emit(&ChildEvent::Notification {
                    notification: if started {
                        NotificationKind::WindowArrangeStarted
                    } else {
                        NotificationKind::WindowArrangeFinished
                    },
                })?;
                // SAFETY: the Shell requested that the AppBar hide during arrangement and show
                // without activation afterwards.
                unsafe {
                    let _was_visible = ShowWindow(
                        self.hwnd.get(),
                        if started { SW_HIDE } else { SW_SHOWNOACTIVATE },
                    );
                };
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

/// Runs one event-driven `AppBar` host on the calling thread.
///
/// # Errors
///
/// Returns an error when registration, positioning, or the Win32 message loop fails.
pub fn run(options: ChildOptions) -> anyhow::Result<()> {
    // SAFETY: requesting the module handle for the current process has no pointer lifetime issue.
    let module = unsafe { GetModuleHandleW(None) }.context("read current module handle")?;
    let class = WNDCLASSW {
        lpfnWndProc: Some(window_proc),
        hInstance: module.into(),
        lpszClassName: WINDOW_CLASS,
        ..Default::default()
    };
    // SAFETY: the class references static class-name storage and a process-lifetime callback.
    if unsafe { RegisterClassW(&raw const class) } == 0 {
        return Err(windows::core::Error::from_thread())
            .context("register AppBar prototype window class");
    }

    let host = Box::new(Host::new(options)?);
    let host = Box::into_raw(host);
    // SAFETY: `host` remains allocated until the message loop exits. WM_NCCREATE records the same
    // pointer in the window. The window starts hidden because WS_VISIBLE is absent.
    let hwnd = match unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            WINDOW_CLASS,
            w!("Komorebi AppBar lifecycle prototype"),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(module.into()),
            Some(host.cast()),
        )
    } {
        Ok(hwnd) => hwnd,
        Err(error) => {
            // SAFETY: window creation failed, so no window owns the allocation.
            drop(unsafe { Box::from_raw(host) });
            return Err(error).context("create hidden AppBar prototype window");
        }
    };

    // SAFETY: `host` is still live and exclusively initialized on this UI thread.
    let host_ref = unsafe { &*host };
    host_ref.hwnd.set(hwnd);
    // SAFETY: the HWND is newly created and valid.
    host_ref.dpi.set(
        NonZeroU32::new(unsafe { GetDpiForWindow(hwnd) }).context("window reported zero DPI")?,
    );
    host_ref.geometry.set(AppBarGeometry {
        thickness: physical_thickness(host_ref.thickness_dip.get(), host_ref.dpi.get())?,
        ..host_ref.geometry.get()
    });
    Host::emit(&ChildEvent::CreatedHidden {
        process_id: std::process::id(),
    })?;
    if is_window_visible(hwnd) {
        bail!("prototype window became visible before registration");
    }
    host_ref.register(PositionReason::Initial)?;

    spawn_command_reader(hwnd);
    let mut message = MSG::default();
    loop {
        // SAFETY: `message` remains valid writable storage for the call.
        let result = unsafe { GetMessageW(&raw mut message, None, 0, 0) };
        if result.0 == -1 {
            return Err(windows::core::Error::from_thread()).context("read AppBar window message");
        }
        if result.0 == 0 {
            break;
        }
        // SAFETY: the message came from this thread's queue.
        unsafe {
            let _translated = TranslateMessage(&raw const message);
            DispatchMessageW(&raw const message);
        }
    }

    // SAFETY: the window has been destroyed and no callback can access its user data. This is the
    // unique reconstruction of the Box created before CreateWindowExW.
    drop(unsafe { Box::from_raw(host) });
    Ok(())
}

fn spawn_command_reader(hwnd: HWND) {
    let hwnd_address = hwnd.0.expose_provenance();
    std::thread::spawn(move || {
        for line in BufReader::new(std::io::stdin().lock()).lines() {
            let Ok(line) = line else {
                report_post_error(post_raw(hwnd_address, SHUTDOWN_MESSAGE, 0));
                return;
            };
            match ChildCommand::parse(&line) {
                Ok(ChildCommand::SetThickness(value)) => {
                    report_post_error(post_raw(hwnd_address, SET_THICKNESS_MESSAGE, value.get()));
                }
                Ok(ChildCommand::SimulateDpi(value)) => {
                    report_post_error(post_raw(hwnd_address, SIMULATE_DPI_MESSAGE, value.get()));
                }
                Ok(ChildCommand::RegisterAgain) => {
                    report_post_error(post_raw(hwnd_address, REGISTER_AGAIN_MESSAGE, 0));
                }
                Ok(ChildCommand::Shutdown) => {
                    report_post_error(post_raw(hwnd_address, SHUTDOWN_MESSAGE, 0));
                    return;
                }
                Err(error) => eprintln!("parse child command: {error}"),
            }
        }
        report_post_error(post_raw(hwnd_address, SHUTDOWN_MESSAGE, 0));
    });
}

fn post_raw(hwnd_address: usize, message: u32, value: u32) -> anyhow::Result<()> {
    let value = usize::try_from(value).context("convert child command payload")?;
    let hwnd = HWND(std::ptr::with_exposed_provenance_mut(hwnd_address));
    // SAFETY: the raw value was copied from a live HWND. Failure means the UI thread has exited.
    unsafe { PostMessageW(Some(hwnd), message, WPARAM(value), LPARAM(0)) }
        .context("post child command")
}

fn report_post_error(result: anyhow::Result<()>) {
    if let Err(error) = result {
        eprintln!("post child command: {error}");
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_NCCREATE {
        // SAFETY: WM_NCCREATE supplies a valid CREATESTRUCTW whose lpCreateParams is the Host
        // pointer passed to CreateWindowExW.
        let create = unsafe { &*(lparam.0 as *const CREATESTRUCTW) };
        let host = create.lpCreateParams.cast::<Host>();
        // SAFETY: this stores the process-owned Host pointer for later callbacks on the same HWND.
        unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, host.expose_provenance().cast_signed()) };
    }

    // SAFETY: GWLP_USERDATA is either zero before WM_NCCREATE or the live Host pointer stored above.
    let host_address = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) }.cast_unsigned();
    let host = std::ptr::with_exposed_provenance_mut::<Host>(host_address);
    if host.is_null() {
        // SAFETY: no host state exists yet, so default processing owns the message.
        return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
    }
    // SAFETY: the Host allocation outlives the window and all callbacks run on this UI thread.
    let host = unsafe { &*host };

    let handled = dispatch_host_message(host, hwnd, message, wparam, lparam);

    match handled {
        Ok(true) => LRESULT(0),
        Ok(false) => {
            // SAFETY: the message was not consumed by the host.
            unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
        }
        Err(error) => {
            let failure = ChildEvent::Failure {
                operation: "window_message".to_owned(),
                message: format!("{error:#}"),
            };
            if let Err(write_error) = Host::emit(&failure) {
                eprintln!("report AppBar failure: {write_error:#}");
            }
            // SAFETY: a failed lifecycle transition must end this disposable host.
            if let Err(destroy_error) = unsafe { DestroyWindow(hwnd) } {
                eprintln!("destroy failed AppBar host: {destroy_error}");
            }
            LRESULT(0)
        }
    }
}

fn dispatch_host_message(
    host: &Host,
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> anyhow::Result<bool> {
    if message == host.callback_message {
        let code = u32::try_from(wparam.0).context("decode AppBar callback")?;
        host.appbar_notification(code, lparam)?;
        return Ok(true);
    }
    if message == host.taskbar_created_message {
        Host::emit(&ChildEvent::Notification {
            notification: NotificationKind::TaskbarCreated,
        })?;
        host.register(PositionReason::ShellRecreated)?;
        return Ok(true);
    }

    match message {
        POSITION_MESSAGE => host.position().map(|()| true),
        SET_THICKNESS_MESSAGE => {
            let value = u32::try_from(wparam.0).context("decode AppBar thickness")?;
            let thickness = NonZeroU32::new(value).context("AppBar thickness is zero")?;
            host.update_thickness(thickness, PositionReason::GeometryChanged)?;
            Ok(true)
        }
        SIMULATE_DPI_MESSAGE => {
            let value = u32::try_from(wparam.0).context("decode simulated DPI")?;
            let dpi = NonZeroU32::new(value).context("simulated DPI is zero")?;
            host.update_dpi(dpi)?;
            Ok(true)
        }
        REGISTER_AGAIN_MESSAGE => host
            .register(PositionReason::GeometryChanged)
            .map(|()| true),
        SHUTDOWN_MESSAGE | WM_CLOSE => host.release().and_then(|()| {
            // SAFETY: this is the owner UI thread and the AppBar has already been released.
            unsafe { DestroyWindow(hwnd) }.context("destroy AppBar window")?;
            Ok(true)
        }),
        WM_DISPLAYCHANGE => host.update_monitor().map(|()| true),
        WM_DPICHANGED => {
            let low_word = u32::try_from(wparam.0 & 0xffff).context("decode window DPI")?;
            let dpi = NonZeroU32::new(low_word).context("window DPI is zero")?;
            host.update_dpi(dpi)?;
            Ok(true)
        }
        WM_ACTIVATE => host.notify_shell(ABM_ACTIVATE).map(|()| false),
        WM_WINDOWPOSCHANGED => host.notify_shell(ABM_WINDOWPOSCHANGED).map(|()| false),
        WM_DESTROY => host.destroy().map(|()| {
            // SAFETY: this ends the current thread's message loop.
            unsafe { PostQuitMessage(0) };
            true
        }),
        WM_NCDESTROY => {
            // SAFETY: no later message may dereference the process-owned Host through this HWND.
            unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) };
            Ok(false)
        }
        _ => Ok(false),
    }
}

const fn edge_value(edge: Edge) -> u32 {
    match edge {
        Edge::Left => ABE_LEFT,
        Edge::Top => ABE_TOP,
        Edge::Right => ABE_RIGHT,
        Edge::Bottom => ABE_BOTTOM,
    }
}

fn physical_thickness(dip: NonZeroU32, dpi: NonZeroU32) -> anyhow::Result<i32> {
    let scaled = i64::from(dip.get())
        .checked_mul(i64::from(dpi.get()))
        .context("scale AppBar thickness")?;
    let rounded = scaled
        .checked_add(i64::from(DEFAULT_DPI / 2))
        .context("round AppBar thickness")?
        / i64::from(DEFAULT_DPI);
    i32::try_from(rounded.max(1)).context("AppBar thickness exceeds Win32 coordinate range")
}
