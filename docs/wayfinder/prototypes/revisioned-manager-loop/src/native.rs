use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::thread;
use std::time::Duration;

use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetForegroundWindow,
    GetWindowRect, IDC_ARROW, IsWindow, IsWindowVisible, LoadCursorW, MSG, PM_REMOVE, PeekMessageW,
    RegisterClassW, SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOZORDER, SetForegroundWindow,
    SetWindowPos, ShowWindow, TranslateMessage, UnregisterClassW, WINDOW_EX_STYLE, WINDOW_STYLE,
    WM_CLOSE, WNDCLASSW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_OVERLAPPEDWINDOW,
    WS_POPUP, WS_VISIBLE,
};
use windows::core::PCWSTR;

use crate::model::{EffectBoundary, EffectOutcome, Geometry, SurfaceFrame, WindowId};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ObservedWindow {
    Present(Geometry),
    Destroyed,
    Unknown(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ObservedSurface {
    Present(SurfaceFrame),
    Unknown(String),
}

pub trait WindowSystem {
    fn focus_window(&mut self, window: WindowId) -> EffectOutcome;
    fn move_window(&mut self, window: WindowId, target: Geometry) -> EffectOutcome;
    fn observe_foreground(&self) -> Option<WindowId>;
    fn observe_window(&self, window: WindowId) -> ObservedWindow;
}

pub trait ShellSurfaceHost {
    fn set_surface(&mut self, target: SurfaceFrame) -> EffectOutcome;
    fn observe_surface(&self) -> ObservedSurface;
}

pub trait NativePorts: WindowSystem + ShellSurfaceHost {}

impl<T> NativePorts for T where T: WindowSystem + ShellSurfaceHost {}

#[derive(Debug)]
pub struct ScriptedNative {
    outcomes: VecDeque<(EffectBoundary, EffectOutcome)>,
    windows: BTreeMap<WindowId, Geometry>,
    destroyed: BTreeSet<WindowId>,
    foreground: Option<WindowId>,
    shell: SurfaceFrame,
}

impl ScriptedNative {
    pub fn new(
        initial_windows: BTreeMap<WindowId, Geometry>,
        shell: SurfaceFrame,
        outcomes: impl IntoIterator<Item = (EffectBoundary, EffectOutcome)>,
    ) -> Self {
        Self {
            outcomes: outcomes.into_iter().collect(),
            windows: initial_windows,
            destroyed: BTreeSet::new(),
            foreground: Some(WindowId(1)),
            shell,
        }
    }

    fn outcome(&mut self, boundary: EffectBoundary) -> EffectOutcome {
        let Some((expected, outcome)) = self.outcomes.pop_front() else {
            return EffectOutcome::Rejected;
        };
        assert_eq!(expected, boundary, "scripted effect boundary changed");
        outcome
    }

    fn may_have_applied(outcome: EffectOutcome) -> bool {
        outcome != EffectOutcome::Rejected
    }
}

impl WindowSystem for ScriptedNative {
    fn focus_window(&mut self, window: WindowId) -> EffectOutcome {
        let outcome = self.outcome(EffectBoundary::FocusWindow);
        if Self::may_have_applied(outcome) && !self.destroyed.contains(&window) {
            self.foreground = Some(window);
        }
        outcome
    }

    fn move_window(&mut self, window: WindowId, target: Geometry) -> EffectOutcome {
        let outcome = self.outcome(EffectBoundary::MoveWindow);
        if Self::may_have_applied(outcome) && !self.destroyed.contains(&window) {
            self.windows.insert(window, target);
        }
        outcome
    }

    fn observe_foreground(&self) -> Option<WindowId> {
        self.foreground
    }

    fn observe_window(&self, window: WindowId) -> ObservedWindow {
        if self.destroyed.contains(&window) {
            ObservedWindow::Destroyed
        } else {
            self.windows
                .get(&window)
                .copied()
                .map_or(ObservedWindow::Destroyed, ObservedWindow::Present)
        }
    }
}

impl ShellSurfaceHost for ScriptedNative {
    fn set_surface(&mut self, target: SurfaceFrame) -> EffectOutcome {
        let outcome = self.outcome(EffectBoundary::ShellSurface);
        if Self::may_have_applied(outcome) {
            self.shell = target;
        }
        outcome
    }

    fn observe_surface(&self) -> ObservedSurface {
        ObservedSurface::Present(self.shell)
    }
}

#[derive(Debug)]
pub struct NativeError(String);

impl Display for NativeError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for NativeError {}

impl From<windows::core::Error> for NativeError {
    fn from(error: windows::core::Error) -> Self {
        Self(error.to_string())
    }
}

pub struct Win32Probe {
    class_name: Vec<u16>,
    instance: HINSTANCE,
    windows: BTreeMap<WindowId, HWND>,
    shell: HWND,
    previous_foreground: HWND,
    last_error: Option<String>,
}

impl Win32Probe {
    pub fn create() -> Result<Self, NativeError> {
        let class_name = wide(&format!(
            "KomorebiRevisionedLoopPrototype-{}",
            std::process::id()
        ));
        // SAFETY: this reads the module handle for the current process and does not retain a
        // borrowed pointer.
        let module = unsafe { GetModuleHandleW(None)? };
        let instance = HINSTANCE(module.0);
        let window_class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            lpszClassName: PCWSTR(class_name.as_ptr()),
            // SAFETY: IDC_ARROW is a system cursor resource with process-independent lifetime.
            hCursor: unsafe { LoadCursorW(None, IDC_ARROW)? },
            ..Default::default()
        };
        // SAFETY: window_class points to storage that remains live through registration; the
        // class name is retained by Win32 and kept alive by Win32Probe until unregistration.
        if unsafe { RegisterClassW(&window_class) } == 0 {
            return Err(NativeError(std::io::Error::last_os_error().to_string()));
        }

        let window_one = create_window(
            instance,
            &class_name,
            "Revision probe workspace 1",
            WINDOW_EX_STYLE::default(),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            Geometry {
                x: 80,
                y: 180,
                width: 360,
                height: 220,
            },
        )?;
        let window_two = create_window(
            instance,
            &class_name,
            "Revision probe workspace 2",
            WINDOW_EX_STYLE::default(),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            Geometry {
                x: 470,
                y: 180,
                width: 360,
                height: 220,
            },
        )?;
        let shell = create_window(
            instance,
            &class_name,
            "Revision probe shell surface",
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
            WS_POPUP,
            Geometry {
                x: 250,
                y: 80,
                width: 420,
                height: 64,
            },
        )?;

        pump_messages();

        Ok(Self {
            class_name,
            instance,
            windows: BTreeMap::from([(WindowId(1), window_one), (WindowId(2), window_two)]),
            shell,
            // SAFETY: GetForegroundWindow has no pointer preconditions and returns a borrowed
            // HWND whose validity is checked before it is reused.
            previous_foreground: unsafe { GetForegroundWindow() },
            last_error: None,
        })
    }

    pub fn window_geometry(&self, window: WindowId) -> Result<Geometry, NativeError> {
        let hwnd = self
            .windows
            .get(&window)
            .copied()
            .ok_or_else(|| NativeError(format!("unknown test window {}", window.0)))?;
        geometry(hwnd)
    }

    pub fn shell_frame(&self) -> Result<SurfaceFrame, NativeError> {
        Ok(SurfaceFrame {
            geometry: geometry(self.shell)?,
            // SAFETY: self.shell is created by this probe and retained until Drop.
            visible: unsafe { IsWindowVisible(self.shell) }.as_bool(),
        })
    }

    pub fn external_move(&mut self, window: WindowId, target: Geometry) -> Result<(), NativeError> {
        let hwnd = self
            .windows
            .get(&window)
            .copied()
            .ok_or_else(|| NativeError(format!("unknown test window {}", window.0)))?;
        // SAFETY: hwnd is a probe-owned handle; SetWindowPos does not retain target.
        unsafe {
            SetWindowPos(
                hwnd,
                None,
                target.x,
                target.y,
                target.width,
                target.height,
                SWP_NOACTIVATE | SWP_NOZORDER,
            )?;
        }
        pump_messages();
        Ok(())
    }

    pub fn external_destroy(&mut self, window: WindowId) -> Result<(), NativeError> {
        let hwnd = self
            .windows
            .get(&window)
            .copied()
            .ok_or_else(|| NativeError(format!("unknown test window {}", window.0)))?;
        // SAFETY: hwnd is a window created and owned by this thread and probe.
        unsafe { DestroyWindow(hwnd)? };
        pump_messages();
        Ok(())
    }

    pub fn take_last_error(&mut self) -> Option<String> {
        self.last_error.take()
    }

    fn remember_error(&mut self, error: windows::core::Error) -> EffectOutcome {
        self.last_error = Some(error.to_string());
        EffectOutcome::Rejected
    }

    fn hwnd(&self, window: WindowId) -> Option<HWND> {
        self.windows.get(&window).copied()
    }
}

impl WindowSystem for Win32Probe {
    fn focus_window(&mut self, window: WindowId) -> EffectOutcome {
        let Some(hwnd) = self.hwnd(window) else {
            self.last_error = Some(format!("unknown test window {}", window.0));
            return EffectOutcome::Rejected;
        };
        // SAFETY: querying a possibly stale HWND is supported; false reports destruction.
        if !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
            self.last_error = Some(format!("test window {} was destroyed", window.0));
            return EffectOutcome::Rejected;
        }

        // SAFETY: hwnd was just validated and belongs to this process.
        let accepted = unsafe { SetForegroundWindow(hwnd) }.as_bool();
        pump_messages();
        thread::sleep(Duration::from_millis(40));
        if !accepted {
            EffectOutcome::Rejected
        // SAFETY: GetForegroundWindow has no pointer preconditions.
        } else if unsafe { GetForegroundWindow() } == hwnd {
            EffectOutcome::Applied
        } else {
            EffectOutcome::Unknown
        }
    }

    fn move_window(&mut self, window: WindowId, target: Geometry) -> EffectOutcome {
        let Some(hwnd) = self.hwnd(window) else {
            self.last_error = Some(format!("unknown test window {}", window.0));
            return EffectOutcome::Rejected;
        };
        // SAFETY: hwnd is probe-owned; the API copies the supplied integer geometry.
        let result = unsafe {
            SetWindowPos(
                hwnd,
                None,
                target.x,
                target.y,
                target.width,
                target.height,
                SWP_NOACTIVATE | SWP_NOZORDER,
            )
        };
        if let Err(error) = result {
            return self.remember_error(error);
        }
        pump_messages();
        match geometry(hwnd) {
            Ok(observed) if observed == target => EffectOutcome::Applied,
            Ok(_) => EffectOutcome::Unknown,
            Err(error) => {
                self.last_error = Some(error.to_string());
                EffectOutcome::Unknown
            }
        }
    }

    fn observe_foreground(&self) -> Option<WindowId> {
        // SAFETY: GetForegroundWindow has no pointer preconditions.
        let foreground = unsafe { GetForegroundWindow() };
        self.windows
            .iter()
            .find_map(|(id, hwnd)| (*hwnd == foreground).then_some(*id))
    }

    fn observe_window(&self, window: WindowId) -> ObservedWindow {
        let Some(hwnd) = self.hwnd(window) else {
            return ObservedWindow::Destroyed;
        };
        // SAFETY: querying a possibly stale HWND is supported; false reports destruction.
        if !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
            return ObservedWindow::Destroyed;
        }
        match geometry(hwnd) {
            Ok(frame) => ObservedWindow::Present(frame),
            Err(error) => ObservedWindow::Unknown(error.to_string()),
        }
    }
}

impl ShellSurfaceHost for Win32Probe {
    fn set_surface(&mut self, target: SurfaceFrame) -> EffectOutcome {
        // SAFETY: self.shell is created by this probe and remains owned until Drop.
        let position = unsafe {
            SetWindowPos(
                self.shell,
                None,
                target.geometry.x,
                target.geometry.y,
                target.geometry.width,
                target.geometry.height,
                SWP_NOACTIVATE | SWP_NOZORDER,
            )
        };
        if let Err(error) = position {
            return self.remember_error(error);
        }
        // SAFETY: self.shell is a live probe-owned HWND; ShowWindow retains no references.
        unsafe {
            let _ = ShowWindow(
                self.shell,
                if target.visible {
                    SW_SHOWNOACTIVATE
                } else {
                    SW_HIDE
                },
            );
        }
        pump_messages();
        match self.shell_frame() {
            Ok(observed) if observed == target => EffectOutcome::Applied,
            Ok(_) => EffectOutcome::Unknown,
            Err(error) => {
                self.last_error = Some(error.to_string());
                EffectOutcome::Unknown
            }
        }
    }

    fn observe_surface(&self) -> ObservedSurface {
        match self.shell_frame() {
            Ok(frame) => ObservedSurface::Present(frame),
            Err(error) => ObservedSurface::Unknown(error.to_string()),
        }
    }
}

impl Drop for Win32Probe {
    fn drop(&mut self) {
        // SAFETY: Drop runs on the creating thread. Every handle is checked before reuse; each
        // window and the registered class are owned by this probe and released exactly once.
        unsafe {
            if !self.previous_foreground.is_invalid()
                && IsWindow(Some(self.previous_foreground)).as_bool()
            {
                let _ = SetForegroundWindow(self.previous_foreground);
            }
            for hwnd in self.windows.values().copied().chain([self.shell]) {
                if IsWindow(Some(hwnd)).as_bool() {
                    let _ = DestroyWindow(hwnd);
                }
            }
            let _ = UnregisterClassW(PCWSTR(self.class_name.as_ptr()), Some(self.instance));
        }
    }
}

fn create_window(
    instance: HINSTANCE,
    class_name: &[u16],
    title: &str,
    extended_style: WINDOW_EX_STYLE,
    style: WINDOW_STYLE,
    frame: Geometry,
) -> Result<HWND, NativeError> {
    let title = wide(title);
    // SAFETY: class/title strings are NUL-terminated and live for the duration of the call;
    // instance and class were registered by this process, and Win32 owns the returned HWND.
    Ok(unsafe {
        CreateWindowExW(
            extended_style,
            PCWSTR(class_name.as_ptr()),
            PCWSTR(title.as_ptr()),
            style,
            frame.x,
            frame.y,
            frame.width,
            frame.height,
            None,
            None,
            Some(instance),
            None,
        )?
    })
}

fn geometry(hwnd: HWND) -> Result<Geometry, NativeError> {
    let mut rect = RECT::default();
    // SAFETY: rect is valid writable storage; callers supply a currently known HWND and handle
    // failure as an unavailable observation rather than assuming destruction.
    unsafe { GetWindowRect(hwnd, &mut rect)? };
    Ok(Geometry {
        x: rect.left,
        y: rect.top,
        width: rect.right - rect.left,
        height: rect.bottom - rect.top,
    })
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain([0]).collect()
}

fn pump_messages() {
    let mut message = MSG::default();
    // SAFETY: message points to valid local storage; the loop owns and dispatches messages for
    // windows created on this thread.
    unsafe {
        while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_CLOSE {
        // SAFETY: WM_CLOSE was delivered to this window procedure for hwnd on its owner thread.
        let _ = unsafe { DestroyWindow(hwnd) };
        LRESULT(0)
    } else {
        // SAFETY: forwarding untouched parameters is the required default Win32 procedure path.
        unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
    }
}
