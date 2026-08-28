use std::cell::Cell;
use std::cmp::Ordering;
use std::ffi::c_void;
use std::mem::zeroed;
use std::ptr::{null, null_mut};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{
    HINSTANCE, HWND, LPARAM, LRESULT, RECT, WAIT_FAILED, WAIT_TIMEOUT, WPARAM,
};
use windows_sys::Win32::Graphics::Dwm::{
    DWM_THUMBNAIL_PROPERTIES, DWM_TNP_OPACITY, DWM_TNP_RECTDESTINATION,
    DWM_TNP_SOURCECLIENTAREAONLY, DWM_TNP_VISIBLE, DwmFlush, DwmRegisterThumbnail,
    DwmUnregisterThumbnail, DwmUpdateThumbnailProperties,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::GetCurrentProcessId;
use windows_sys::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow,
    DispatchMessageW, EVENT_SYSTEM_FOREGROUND, EnumWindows, GWL_EXSTYLE, GWL_STYLE,
    GetForegroundWindow, GetWindowLongW, GetWindowTextLengthW, GetWindowThreadProcessId, IDC_ARROW,
    IsWindowVisible, LoadCursorW, MSG, MWMO_INPUTAVAILABLE, MsgWaitForMultipleObjectsEx, PM_REMOVE,
    PeekMessageW, QS_ALLINPUT, RegisterClassW, SW_SHOW, SetForegroundWindow, ShowWindow,
    TranslateMessage, UnregisterClassW, WINEVENT_OUTOFCONTEXT, WNDCLASSW, WS_EX_APPWINDOW,
    WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
};

const WIDTH: i32 = 1600;
const HEIGHT: i32 = 900;
const SAMPLES: usize = 120;

thread_local! {
    // WINEVENT_OUTOFCONTEXT callbacks are delivered on the installing thread.
    static EXPECTED_FOREGROUND: Cell<HWND> = const { Cell::new(null_mut()) };
    static OBSERVED_FOREGROUND: Cell<HWND> = const { Cell::new(null_mut()) };
}

#[derive(Clone, Copy)]
struct Source(HWND);

struct Thumbnail(isize);

impl Drop for Thumbnail {
    fn drop(&mut self) {
        // SAFETY: this wrapper exclusively owns a successful DWM registration.
        unsafe { DwmUnregisterThumbnail(self.0) };
    }
}

struct Window(HWND);

impl Drop for Window {
    fn drop(&mut self) {
        // SAFETY: this wrapper exclusively owns a successful CreateWindowExW result.
        unsafe { DestroyWindow(self.0) };
    }
}

struct WindowClass {
    name: Vec<u16>,
    instance: HINSTANCE,
}

impl Drop for WindowClass {
    fn drop(&mut self) {
        // SAFETY: name and instance identify the class registered by this wrapper.
        unsafe { UnregisterClassW(self.name.as_ptr(), self.instance) };
    }
}

struct ForegroundHook(HWINEVENTHOOK);

impl Drop for ForegroundHook {
    fn drop(&mut self) {
        // SAFETY: this wrapper exclusively owns a successful SetWinEventHook result.
        unsafe { UnhookWinEvent(self.0) };
    }
}

#[derive(Debug)]
struct Workload {
    slots: usize,
    source_windows: usize,
    register_ms: f64,
    registration_flush_ms: f64,
    update_p50_ms: f64,
    update_p95_ms: f64,
    update_max_ms: f64,
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain([0]).collect()
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY: unhandled messages are forwarded with the exact values supplied by Windows.
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

unsafe extern "system" fn foreground_changed(
    _hook: HWINEVENTHOOK,
    event: u32,
    window: HWND,
    _object: i32,
    _child: i32,
    _event_thread: u32,
    _event_time: u32,
) {
    let expected = EXPECTED_FOREGROUND.get();
    if event == EVENT_SYSTEM_FOREGROUND && window == expected {
        OBSERVED_FOREGROUND.set(window);
    }
}

unsafe extern "system" fn collect_window(window: HWND, state: LPARAM) -> i32 {
    let mut process_id = 0;
    // SAFETY: process_id points to initialized writable storage for the duration of the call.
    unsafe { GetWindowThreadProcessId(window, &mut process_id) };
    // SAFETY: state was created from a live mutable Vec<Source> for the synchronous EnumWindows call.
    let sources = unsafe { &mut *(state as *mut Vec<Source>) };
    // SAFETY: these queries accept any top-level HWND supplied by EnumWindows.
    let visible = unsafe { IsWindowVisible(window) != 0 };
    // SAFETY: GetWindowTextLengthW accepts the HWND supplied by EnumWindows.
    let titled = unsafe { GetWindowTextLengthW(window) > 0 };
    // SAFETY: style queries accept the HWND supplied by EnumWindows.
    let style = unsafe { GetWindowLongW(window, GWL_STYLE) as u32 };
    // SAFETY: extended-style queries accept the HWND supplied by EnumWindows.
    let ex_style = unsafe { GetWindowLongW(window, GWL_EXSTYLE) as u32 };
    // SAFETY: retrieving the current process id has no preconditions.
    let current_process = unsafe { GetCurrentProcessId() };

    if visible
        && titled
        && process_id != current_process
        && style & WS_VISIBLE != 0
        && ex_style & WS_EX_TOOLWINDOW == 0
    {
        sources.push(Source(window));
    }
    1
}

fn sources() -> Vec<Source> {
    let mut sources = Vec::new();
    // SAFETY: callback does not escape, and the LPARAM points to sources for the synchronous call.
    unsafe {
        EnumWindows(
            Some(collect_window),
            (&mut sources as *mut Vec<Source>) as LPARAM,
        )
    };
    sources
}

fn register_window_class() -> Result<WindowClass, String> {
    let class_name = wide("WayfinderOverviewNativeProbe");
    // SAFETY: null requests the module handle of the current process.
    let instance = unsafe { GetModuleHandleW(null()) } as HINSTANCE;
    // SAFETY: null requests the system arrow cursor resource.
    let cursor = unsafe { LoadCursorW(null_mut(), IDC_ARROW) };
    // SAFETY: WNDCLASSW permits zero initialization before required fields are assigned.
    let empty_class: WNDCLASSW = unsafe { zeroed() };
    let class = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(window_proc),
        hInstance: instance,
        lpszClassName: class_name.as_ptr(),
        hCursor: cursor,
        ..empty_class
    };
    // SAFETY: class points to process-lifetime strings and a valid window procedure.
    if unsafe { RegisterClassW(&class) } == 0 {
        return Err("RegisterClassW failed".into());
    }

    Ok(WindowClass {
        name: class_name,
        instance,
    })
}

fn create_destination(class: &WindowClass) -> Result<Window, String> {
    let title = wide("Wayfinder overview native probe");
    // SAFETY: every pointer is null or points to a live zero-terminated UTF-16 string.
    let window = unsafe {
        CreateWindowExW(
            WS_EX_APPWINDOW | WS_EX_NOREDIRECTIONBITMAP,
            class.name.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            WIDTH,
            HEIGHT,
            null_mut(),
            null_mut(),
            class.instance,
            null_mut::<c_void>(),
        )
    };
    if window.is_null() {
        Err("CreateWindowExW failed".into())
    } else {
        Ok(Window(window))
    }
}

fn install_foreground_hook() -> Result<ForegroundHook, String> {
    // SAFETY: callback is process-lifetime code; out-of-context delivery needs no injected DLL.
    let hook = unsafe {
        SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            null_mut(),
            Some(foreground_changed),
            0,
            0,
            WINEVENT_OUTOFCONTEXT,
        )
    };
    if hook.is_null() {
        Err("SetWinEventHook(EVENT_SYSTEM_FOREGROUND) failed".into())
    } else {
        Ok(ForegroundHook(hook))
    }
}

fn dispatch_ready_messages() {
    // This drains only after Windows reports queued input; it is event dispatch, not polling.
    // SAFETY: message storage is initialized and valid for the duration of each call.
    unsafe {
        let mut message: MSG = zeroed();
        while PeekMessageW(&mut message, null_mut(), 0, 0, PM_REMOVE) != 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

fn wait_for_foreground_event(expected: HWND, timeout: Duration) -> Result<bool, String> {
    let deadline = Instant::now() + timeout;
    loop {
        if OBSERVED_FOREGROUND.get() == expected {
            return Ok(true);
        }

        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Ok(false);
        }
        let timeout_ms = remaining.as_millis().clamp(1, u32::MAX as u128) as u32;
        // SAFETY: zero handles and a null handle array ask Windows to wait for queued input.
        let result = unsafe {
            MsgWaitForMultipleObjectsEx(0, null(), timeout_ms, QS_ALLINPUT, MWMO_INPUTAVAILABLE)
        };
        if result == WAIT_TIMEOUT {
            return Ok(false);
        }
        if result == WAIT_FAILED {
            return Err("MsgWaitForMultipleObjectsEx failed".into());
        }
        dispatch_ready_messages();
    }
}

fn activate_and_observe(window: HWND, timeout: Duration) -> Result<(bool, bool), String> {
    OBSERVED_FOREGROUND.set(null_mut());
    EXPECTED_FOREGROUND.set(window);
    // SAFETY: window is a currently live top-level window.
    let accepted = unsafe { SetForegroundWindow(window) != 0 };
    // No foreground event is required when the requested window was already foreground.
    // SAFETY: GetForegroundWindow has no preconditions.
    let already_foreground = unsafe { GetForegroundWindow() == window };
    let observed = already_foreground || wait_for_foreground_event(window, timeout)?;
    Ok((accepted, observed))
}

fn percentile(samples: &[f64], ratio: f64) -> f64 {
    let mut ordered = samples.to_vec();
    ordered.sort_by(|left, right| left.partial_cmp(right).unwrap_or(Ordering::Equal));
    ordered[((ordered.len() - 1) as f64 * ratio).round() as usize]
}

fn properties(slot: usize, slots: usize, opacity: u8) -> DWM_THUMBNAIL_PROPERTIES {
    let columns = if slots <= 20 { 5 } else { 10 };
    let rows = slots.div_ceil(columns);
    let gap = 8;
    let top = 44;
    let cell_width = (WIDTH - gap * (columns as i32 + 1)) / columns as i32;
    let cell_height = (HEIGHT - top - gap * (rows as i32 + 1)) / rows as i32;
    let column = slot % columns;
    let row = slot / columns;
    let left = gap + column as i32 * (cell_width + gap);
    let top = top + gap + row as i32 * (cell_height + gap);

    DWM_THUMBNAIL_PROPERTIES {
        dwFlags: DWM_TNP_RECTDESTINATION
            | DWM_TNP_OPACITY
            | DWM_TNP_VISIBLE
            | DWM_TNP_SOURCECLIENTAREAONLY,
        rcDestination: RECT {
            left,
            top,
            right: left + cell_width,
            bottom: top + cell_height,
        },
        rcSource: RECT::default(),
        opacity,
        fVisible: 1,
        fSourceClientAreaOnly: 0,
    }
}

fn register_workload(
    destination: HWND,
    sources: &[Source],
    slots: usize,
) -> Result<Workload, String> {
    let started = Instant::now();
    let mut thumbnails = Vec::with_capacity(slots);
    for slot in 0..slots {
        let mut handle = 0;
        // SAFETY: destination is caller-owned, source is an enumerated top-level window, output is writable.
        let result = unsafe {
            DwmRegisterThumbnail(destination, sources[slot % sources.len()].0, &mut handle)
        };
        if result < 0 {
            return Err(format!("DwmRegisterThumbnail failed: 0x{result:08x}"));
        }
        // SAFETY: handle is a successful registration owned by this scope; properties are initialized.
        let result = unsafe { DwmUpdateThumbnailProperties(handle, &properties(slot, slots, 255)) };
        if result < 0 {
            return Err(format!(
                "DwmUpdateThumbnailProperties failed: 0x{result:08x}"
            ));
        }
        thumbnails.push(Thumbnail(handle));
    }
    let register_ms = started.elapsed().as_secs_f64() * 1000.0;
    // SAFETY: waits only for DWM work queued by this process.
    let result = unsafe { DwmFlush() };
    if result < 0 {
        return Err(format!("DwmFlush failed: 0x{result:08x}"));
    }
    let registration_flush_ms = started.elapsed().as_secs_f64() * 1000.0;

    let mut update_ms = Vec::with_capacity(SAMPLES);
    for sample in 0..SAMPLES {
        let started = Instant::now();
        let opacity = if sample % 2 == 0 { 255 } else { 254 };
        for (slot, thumbnail) in thumbnails.iter().enumerate() {
            // SAFETY: each thumbnail remains registered and properties are initialized.
            let result = unsafe {
                DwmUpdateThumbnailProperties(thumbnail.0, &properties(slot, slots, opacity))
            };
            if result < 0 {
                return Err(format!("thumbnail update failed: 0x{result:08x}"));
            }
        }
        // SAFETY: waits only for DWM work queued by this process.
        let result = unsafe { DwmFlush() };
        if result < 0 {
            return Err(format!("DwmFlush failed: 0x{result:08x}"));
        }
        update_ms.push(started.elapsed().as_secs_f64() * 1000.0);
    }

    Ok(Workload {
        slots,
        source_windows: sources.len(),
        register_ms,
        registration_flush_ms,
        update_p50_ms: percentile(&update_ms, 0.50),
        update_p95_ms: percentile(&update_ms, 0.95),
        update_max_ms: percentile(&update_ms, 1.0),
    })
}

fn main() -> Result<(), String> {
    let sources = sources();
    if sources.is_empty() {
        return Err("no eligible visible source windows".into());
    }
    let class = register_window_class()?;
    let destination = create_destination(&class)?;
    let _foreground_hook = install_foreground_hook()?;
    // SAFETY: destination is a valid caller-owned top-level window.
    unsafe {
        ShowWindow(destination.0, SW_SHOW);
    }

    let twenty = register_workload(destination.0, &sources, 20)?;
    let fifty = register_workload(destination.0, &sources, 50)?;

    // The production path is initiated by the hotkey and owns foreground input while the overview is open.
    let (_, overview_foreground) = activate_and_observe(destination.0, Duration::from_millis(100))?;
    let activation_started = Instant::now();
    let (activation_call, activated) =
        activate_and_observe(sources[0].0, Duration::from_millis(100))?;
    let activation_ms = activation_started.elapsed().as_secs_f64() * 1000.0;

    println!(
        concat!(
            "{{\n",
            "  \"source_window_count\": {},\n",
            "  \"activation\": {{\"overview_foreground\": {}, \"call_accepted\": {}, \"target_became_foreground\": {}, \"latency_ms\": {:.3}}},\n",
            "  \"workloads\": [\n",
            "    {{\"slots\": {}, \"source_windows\": {}, \"register_ms\": {:.3}, \"registration_flush_ms\": {:.3}, \"update_p50_ms\": {:.3}, \"update_p95_ms\": {:.3}, \"update_max_ms\": {:.3}}},\n",
            "    {{\"slots\": {}, \"source_windows\": {}, \"register_ms\": {:.3}, \"registration_flush_ms\": {:.3}, \"update_p50_ms\": {:.3}, \"update_p95_ms\": {:.3}, \"update_max_ms\": {:.3}}}\n",
            "  ]\n",
            "}}"
        ),
        sources.len(),
        overview_foreground,
        activation_call,
        activated,
        activation_ms,
        twenty.slots,
        twenty.source_windows,
        twenty.register_ms,
        twenty.registration_flush_ms,
        twenty.update_p50_ms,
        twenty.update_p95_ms,
        twenty.update_max_ms,
        fifty.slots,
        fifty.source_windows,
        fifty.register_ms,
        fifty.registration_flush_ms,
        fifty.update_p50_ms,
        fifty.update_p95_ms,
        fifty.update_max_ms,
    );
    Ok(())
}
