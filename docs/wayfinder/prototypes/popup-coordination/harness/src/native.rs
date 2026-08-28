use std::ffi::c_void;
use std::mem::size_of;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use serde::Serialize;
use sha2::Digest;
use sha2::Sha256;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::Foundation::FILETIME;
use windows::Win32::Foundation::HWND;
use windows::Win32::Foundation::LPARAM;
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Dwm::DWMWA_CLOAKED;
use windows::Win32::Graphics::Dwm::DWMWA_EXTENDED_FRAME_BOUNDS;
use windows::Win32::Graphics::Dwm::DwmGetWindowAttribute;
use windows::Win32::Graphics::Gdi::GetMonitorInfoW;
use windows::Win32::Graphics::Gdi::MONITOR_DEFAULTTONEAREST;
use windows::Win32::Graphics::Gdi::MONITORINFO;
use windows::Win32::Graphics::Gdi::MonitorFromWindow;
use windows::Win32::System::Threading::GetProcessTimes;
use windows::Win32::System::Threading::OpenProcess;
use windows::Win32::System::Threading::PROCESS_QUERY_LIMITED_INFORMATION;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::IsWindowEnabled;
use windows::Win32::UI::WindowsAndMessaging::EnumWindows;
use windows::Win32::UI::WindowsAndMessaging::GA_ROOTOWNER;
use windows::Win32::UI::WindowsAndMessaging::GW_HWNDNEXT;
use windows::Win32::UI::WindowsAndMessaging::GW_HWNDPREV;
use windows::Win32::UI::WindowsAndMessaging::GW_OWNER;
use windows::Win32::UI::WindowsAndMessaging::GWL_EXSTYLE;
use windows::Win32::UI::WindowsAndMessaging::GWL_STYLE;
use windows::Win32::UI::WindowsAndMessaging::GetAncestor;
use windows::Win32::UI::WindowsAndMessaging::GetClassNameW;
use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
use windows::Win32::UI::WindowsAndMessaging::GetWindow;
use windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW;
use windows::Win32::UI::WindowsAndMessaging::GetWindowRect;
use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
use windows::Win32::UI::WindowsAndMessaging::IsWindow;
use windows::Win32::UI::WindowsAndMessaging::IsWindowVisible;
use windows::Win32::UI::WindowsAndMessaging::SET_WINDOW_POS_FLAGS;
use windows::Win32::UI::WindowsAndMessaging::SWP_ASYNCWINDOWPOS;
use windows::Win32::UI::WindowsAndMessaging::SWP_NOACTIVATE;
use windows::Win32::UI::WindowsAndMessaging::SWP_NOOWNERZORDER;
use windows::Win32::UI::WindowsAndMessaging::SWP_NOSIZE;
use windows::Win32::UI::WindowsAndMessaging::SWP_NOZORDER;
use windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow;
use windows::Win32::UI::WindowsAndMessaging::SetWindowPos;
use windows::Win32::UI::WindowsAndMessaging::WS_EX_APPWINDOW;
use windows::Win32::UI::WindowsAndMessaging::WS_EX_DLGMODALFRAME;
use windows::Win32::UI::WindowsAndMessaging::WS_EX_NOACTIVATE;
use windows::Win32::UI::WindowsAndMessaging::WS_EX_TOOLWINDOW;
use windows::Win32::UI::WindowsAndMessaging::WS_EX_TOPMOST;

use crate::domain::EnabledState;
use crate::domain::PhysicalRect;
use crate::domain::PlacementPlan;
use crate::domain::StyleEvidence;
use crate::domain::StyleFlag;
use crate::domain::Visibility;
use crate::domain::WindowId;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeWindowRef(HWND);

impl NativeWindowRef {
    pub fn from_raw(raw: isize) -> Result<Self> {
        let address = usize::try_from(raw).context("negative window address")?;
        let window = HWND(std::ptr::with_exposed_provenance_mut::<c_void>(address));
        // SAFETY: IsWindow accepts arbitrary HWND values and performs the validity check itself.
        if !unsafe { IsWindow(Some(window)) }.as_bool() {
            bail!("native window reference is stale");
        }
        Ok(Self(window))
    }

    pub fn raw(self) -> isize {
        isize::try_from(self.0.0.addr()).unwrap_or(isize::MAX)
    }

    pub(crate) const fn hwnd(self) -> HWND {
        self.0
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Win32Observation {
    pub window: isize,
    pub stable_id: WindowId,
    pub process_id: u32,
    pub process_created_100ns: u64,
    pub source_thread_id: u32,
    pub generation: u64,
    pub owner: Option<isize>,
    pub root_owner: Option<isize>,
    pub style: u32,
    pub extended_style: u32,
    pub style_evidence: StyleEvidence,
    pub visibility: Visibility,
    pub enabled: EnabledState,
    pub foreground: ForegroundState,
    pub cloaked: ResultValue<bool>,
    pub frame: ResultValue<PhysicalRect>,
    pub monitor: Option<isize>,
    pub dpi: ResultValue<u32>,
    pub work_area: ResultValue<PhysicalRect>,
    pub z_previous: Option<isize>,
    pub z_next: Option<isize>,
    pub class_utf16: Vec<u16>,
    pub class_truncated: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "snake_case", tag = "availability", content = "value")]
pub enum ResultValue<T> {
    Known(T),
    Unavailable(String),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ForegroundState {
    Foreground,
    Background,
}

#[derive(Clone, Copy, Debug)]
pub struct ControlledWindow {
    window: NativeWindowRef,
    process_id: u32,
}

impl ControlledWindow {
    pub fn verify(window: NativeWindowRef, expected_process_id: u32) -> Result<Self> {
        let mut actual_process_id = 0_u32;
        // SAFETY: The HWND was validated and the process-id pointer is valid writable storage.
        unsafe {
            GetWindowThreadProcessId(window.hwnd(), Some(&raw mut actual_process_id));
        }
        if actual_process_id != expected_process_id {
            bail!("window is not owned by the controlled producer");
        }
        Ok(Self {
            window,
            process_id: expected_process_id,
        })
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct PlacementOutcome {
    pub before: Win32Observation,
    pub after: Win32Observation,
    pub invariants: Vec<PlacementInvariantEvidence>,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct PlacementInvariantEvidence {
    pub invariant: PlacementInvariant,
    pub preserved: bool,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlacementInvariant {
    Size,
    Owner,
    Styles,
    Topmost,
    ZNeighbors,
    Foreground,
}

pub fn observe_window(window: NativeWindowRef, generation: u64) -> Result<Win32Observation> {
    let hwnd = window.hwnd();
    // SAFETY: IsWindow accepts arbitrary HWND values and performs the validity check itself.
    if !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
        bail!("window disappeared before observation");
    }
    let mut process_id = 0_u32;
    // SAFETY: The HWND is live and the process-id pointer is valid writable storage.
    let source_thread_id = unsafe { GetWindowThreadProcessId(hwnd, Some(&raw mut process_id)) };
    if process_id == 0 || source_thread_id == 0 {
        bail!("window has no observable process incarnation");
    }
    let process_created_100ns = process_creation_time(process_id)?;
    // SAFETY: The HWND was revalidated above; both indices request documented style words.
    let style = u32::try_from(unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) })
        .context("window style did not fit u32")?;
    // SAFETY: The HWND was revalidated above; the index requests the extended style word.
    let extended_style = u32::try_from(unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) })
        .context("extended window style did not fit u32")?;
    let frame = dwm_frame(hwnd).or_else(|_| window_rect(hwnd)).map_or_else(
        |error| ResultValue::Unavailable(format!("{error:#}")),
        ResultValue::Known,
    );
    // SAFETY: The HWND is live and the fallback flag guarantees a nearest monitor when available.
    let monitor_handle = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    let monitor = (!monitor_handle.0.is_null())
        .then(|| isize::try_from(monitor_handle.0.addr()).unwrap_or(isize::MAX));
    let work_area = monitor_work_area(monitor_handle).map_or_else(
        |error| ResultValue::Unavailable(format!("{error:#}")),
        ResultValue::Known,
    );
    // SAFETY: The HWND was revalidated above; zero is handled as unavailable evidence.
    let dpi_value = unsafe { GetDpiForWindow(hwnd) };
    let dpi = if dpi_value == 0 {
        ResultValue::Unavailable("GetDpiForWindow returned zero".to_owned())
    } else {
        ResultValue::Known(dpi_value)
    };
    let (class_utf16, class_truncated) = class_name(hwnd);
    Ok(Win32Observation {
        window: window.raw(),
        stable_id: stable_window_id(process_id, process_created_100ns, window.raw(), generation),
        process_id,
        process_created_100ns,
        source_thread_id,
        generation,
        owner: related_window(hwnd, GW_OWNER),
        // SAFETY: The HWND is live and GA_ROOTOWNER is a documented ancestor query.
        root_owner: optional_raw(unsafe { GetAncestor(hwnd, GA_ROOTOWNER) }),
        style,
        extended_style,
        style_evidence: style_evidence(extended_style),
        // SAFETY: The HWND was revalidated above.
        visibility: if unsafe { IsWindowVisible(hwnd) }.as_bool() {
            Visibility::Visible
        } else {
            Visibility::Hidden
        },
        // SAFETY: The HWND was revalidated above.
        enabled: if unsafe { IsWindowEnabled(hwnd) }.as_bool() {
            EnabledState::Enabled
        } else {
            EnabledState::Disabled
        },
        // SAFETY: GetForegroundWindow takes no pointers and returns a borrowed HWND value.
        foreground: if unsafe { GetForegroundWindow() } == hwnd {
            ForegroundState::Foreground
        } else {
            ForegroundState::Background
        },
        cloaked: dwm_cloaked(hwnd).map_or_else(
            |error| ResultValue::Unavailable(format!("{error:#}")),
            ResultValue::Known,
        ),
        frame,
        monitor,
        dpi,
        work_area,
        z_previous: related_window(hwnd, GW_HWNDPREV),
        z_next: related_window(hwnd, GW_HWNDNEXT),
        class_utf16,
        class_truncated,
    })
}

unsafe extern "system" fn collect_visible_window(
    window: HWND,
    context: LPARAM,
) -> windows::core::BOOL {
    // SAFETY: EnumWindows supplies a valid enumerated HWND.
    if unsafe { IsWindowVisible(window) }.as_bool() {
        let address = usize::try_from(context.0).unwrap_or(usize::MAX);
        let windows = std::ptr::with_exposed_provenance_mut::<Vec<NativeWindowRef>>(address);
        // SAFETY: EnumWindows is synchronous and context points to `windows` for this call.
        unsafe { &mut *windows }.push(NativeWindowRef(window));
    }
    true.into()
}

pub fn census_visible_top_level() -> Result<Vec<NativeWindowRef>> {
    let mut windows = Vec::<NativeWindowRef>::new();
    let address = (&raw mut windows).cast::<c_void>().addr();
    let context = LPARAM(isize::try_from(address).context("enumeration context overflow")?);
    // SAFETY: The callback ABI is correct and `context` remains valid for synchronous enumeration.
    unsafe { EnumWindows(Some(collect_visible_window), context) }?;
    Ok(windows)
}

pub fn apply_controlled(
    controlled: ControlledWindow,
    plan: PlacementPlan,
) -> Result<Win32Observation> {
    let before = observe_window(controlled.window, plan.generation.0)?;
    if before.process_id != controlled.process_id || before.stable_id != plan.window {
        bail!("placement plan targets a stale window incarnation");
    }
    let flags: SET_WINDOW_POS_FLAGS =
        SWP_ASYNCWINDOWPOS | SWP_NOACTIVATE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOOWNERZORDER;
    // SAFETY: The controlled HWND and incarnation were revalidated; flags preserve forbidden state.
    unsafe {
        SetWindowPos(
            controlled.window.hwnd(),
            None,
            plan.target.left,
            plan.target.top,
            0,
            0,
            flags,
        )?;
    }
    Ok(before)
}

pub fn verify_placement(before: Win32Observation, after: Win32Observation) -> PlacementOutcome {
    PlacementOutcome {
        invariants: vec![
            PlacementInvariantEvidence {
                invariant: PlacementInvariant::Size,
                preserved: dimensions(&before.frame) == dimensions(&after.frame),
            },
            PlacementInvariantEvidence {
                invariant: PlacementInvariant::Owner,
                preserved: before.owner == after.owner && before.root_owner == after.root_owner,
            },
            PlacementInvariantEvidence {
                invariant: PlacementInvariant::Styles,
                preserved: before.style == after.style
                    && before.extended_style == after.extended_style,
            },
            PlacementInvariantEvidence {
                invariant: PlacementInvariant::Topmost,
                preserved: before.style_evidence.has(StyleFlag::Topmost)
                    == after.style_evidence.has(StyleFlag::Topmost),
            },
            PlacementInvariantEvidence {
                invariant: PlacementInvariant::ZNeighbors,
                preserved: before.z_previous == after.z_previous && before.z_next == after.z_next,
            },
            PlacementInvariantEvidence {
                invariant: PlacementInvariant::Foreground,
                preserved: before.foreground == after.foreground,
            },
        ],
        before,
        after,
    }
}

fn style_evidence(extended_style: u32) -> StyleEvidence {
    [
        (WS_EX_DLGMODALFRAME.0, StyleFlag::DialogFrame),
        (WS_EX_TOOLWINDOW.0, StyleFlag::ToolWindow),
        (WS_EX_NOACTIVATE.0, StyleFlag::NoActivate),
        (WS_EX_APPWINDOW.0, StyleFlag::AppWindow),
        (WS_EX_TOPMOST.0, StyleFlag::Topmost),
    ]
    .into_iter()
    .filter(|(mask, _)| extended_style & mask != 0)
    .fold(StyleEvidence::EMPTY, |evidence, (_, flag)| {
        evidence.with(flag)
    })
}

pub fn request_foreground_once(controlled: ControlledWindow) -> bool {
    // SAFETY: The token proves the HWND belongs to the disposable controlled producer.
    unsafe { SetForegroundWindow(controlled.window.hwnd()) }.as_bool()
}

fn process_creation_time(process_id: u32) -> Result<u64> {
    // SAFETY: The access mask is read-only and `process_id` came from User32.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id) }?;
    let mut creation = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    // SAFETY: `process` is open and all FILETIME pointers are valid writable storage.
    let result = unsafe {
        GetProcessTimes(
            process,
            &raw mut creation,
            &raw mut exit,
            &raw mut kernel,
            &raw mut user,
        )
    };
    // SAFETY: This scope uniquely owns the process handle and closes it exactly once.
    let close_result = unsafe { CloseHandle(process) };
    result?;
    close_result?;
    Ok((u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime))
}

fn stable_window_id(
    process_id: u32,
    process_created_100ns: u64,
    window: isize,
    generation: u64,
) -> WindowId {
    let mut hash = Sha256::new();
    hash.update(process_id.to_le_bytes());
    hash.update(process_created_100ns.to_le_bytes());
    hash.update(window.to_le_bytes());
    hash.update(generation.to_le_bytes());
    WindowId(hash.finalize().into())
}

pub(crate) fn incarnation_id_for_proof(
    process_id: u32,
    process_created_100ns: u64,
    window: isize,
    generation: u64,
) -> WindowId {
    stable_window_id(process_id, process_created_100ns, window, generation)
}

fn class_name(window: HWND) -> (Vec<u16>, bool) {
    const CAPACITY: usize = 1_024;
    let mut buffer = [0_u16; CAPACITY];
    // SAFETY: `buffer` is writable for the call and the HWND came from a live observation.
    let length = unsafe { GetClassNameW(window, &mut buffer) };
    let used = usize::try_from(length).unwrap_or(0).min(CAPACITY);
    (buffer[..used].to_vec(), used == CAPACITY - 1)
}

fn dwm_frame(window: HWND) -> Result<PhysicalRect> {
    let mut rect = RECT::default();
    // SAFETY: The attribute type matches RECT and the pointer/size cover exactly that storage.
    unsafe {
        DwmGetWindowAttribute(
            window,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            (&raw mut rect).cast(),
            u32::try_from(size_of::<RECT>())?,
        )?;
    }
    rect_to_domain(rect)
}

fn window_rect(window: HWND) -> Result<PhysicalRect> {
    let mut rect = RECT::default();
    // SAFETY: `rect` is valid writable storage and the caller supplied a live HWND.
    unsafe { GetWindowRect(window, &raw mut rect) }?;
    rect_to_domain(rect)
}

fn dwm_cloaked(window: HWND) -> Result<bool> {
    let mut cloaked = 0_u32;
    // SAFETY: The attribute type matches u32 and the pointer/size cover exactly that storage.
    unsafe {
        DwmGetWindowAttribute(
            window,
            DWMWA_CLOAKED,
            (&raw mut cloaked).cast(),
            u32::try_from(size_of::<u32>())?,
        )?;
    }
    Ok(cloaked != 0)
}

fn monitor_work_area(monitor: windows::Win32::Graphics::Gdi::HMONITOR) -> Result<PhysicalRect> {
    if monitor.0.is_null() {
        bail!("window has no monitor");
    }
    let mut info = MONITORINFO {
        cbSize: u32::try_from(size_of::<MONITORINFO>())?,
        ..Default::default()
    };
    // SAFETY: The monitor is non-null and MONITORINFO carries its required initialized size.
    if !unsafe { GetMonitorInfoW(monitor, &raw mut info) }.as_bool() {
        return Err(windows::core::Error::from_thread().into());
    }
    rect_to_domain(info.rcWork)
}

fn rect_to_domain(rect: RECT) -> Result<PhysicalRect> {
    PhysicalRect::new(rect.left, rect.top, rect.right, rect.bottom).map_err(Into::into)
}

fn related_window(
    window: HWND,
    command: windows::Win32::UI::WindowsAndMessaging::GET_WINDOW_CMD,
) -> Option<isize> {
    // SAFETY: The caller supplies a live HWND and one of the documented relationship commands.
    // Null is a documented "no related window" result; the generated wrapper represents it as
    // Err, so `.ok()` intentionally maps that absence to None.
    unsafe { GetWindow(window, command) }
        .ok()
        .and_then(optional_raw)
}

fn optional_raw(window: HWND) -> Option<isize> {
    (!window.0.is_null()).then(|| isize::try_from(window.0.addr()).unwrap_or(isize::MAX))
}

fn dimensions(frame: &ResultValue<PhysicalRect>) -> Option<(i32, i32)> {
    match frame {
        ResultValue::Known(rect) => Some((rect.right - rect.left, rect.bottom - rect.top)),
        ResultValue::Unavailable(_) => None,
    }
}
