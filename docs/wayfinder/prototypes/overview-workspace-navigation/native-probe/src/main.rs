use std::cmp::Ordering;
use std::ffi::c_void;
use std::mem::zeroed;
use std::ptr::{null, null_mut};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::{
    DWM_THUMBNAIL_PROPERTIES, DWM_TNP_OPACITY, DWM_TNP_RECTDESTINATION,
    DWM_TNP_SOURCECLIENTAREAONLY, DWM_TNP_VISIBLE, DwmFlush, DwmRegisterThumbnail,
    DwmUnregisterThumbnail, DwmUpdateThumbnailProperties,
};
use windows_sys::Win32::Graphics::Gdi::{CreateSolidBrush, HBRUSH};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::GetCurrentProcessId;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow,
    DispatchMessageW, EnumWindows, GWL_EXSTYLE, GWL_STYLE, GetForegroundWindow, GetWindowLongW,
    GetWindowTextLengthW, GetWindowThreadProcessId, IDC_ARROW, IsWindowVisible, LoadCursorW, MSG,
    PM_REMOVE, PeekMessageW, PostQuitMessage, RegisterClassW, SW_SHOW, SetForegroundWindow,
    ShowWindow, TranslateMessage, WM_DESTROY, WNDCLASSW, WS_EX_APPWINDOW,
    WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
};

const WIDTH: i32 = 1600;
const HEIGHT: i32 = 900;
const SAMPLES: usize = 120;

#[derive(Clone, Copy)]
struct Source(HWND);

struct Thumbnail(isize);

impl Drop for Thumbnail {
    fn drop(&mut self) {
        // SAFETY: this wrapper exclusively owns a successful DWM registration.
        unsafe { DwmUnregisterThumbnail(self.0) };
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
    if message == WM_DESTROY {
        // SAFETY: posting a quit message has no additional preconditions.
        unsafe { PostQuitMessage(0) };
        return 0;
    }

    // SAFETY: unhandled messages are forwarded with the exact values supplied by Windows.
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
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

fn create_destination() -> Result<HWND, String> {
    let class_name = wide("WayfinderOverviewNativeProbe");
    let title = wide("Wayfinder overview native probe");
    // SAFETY: null requests the module handle of the current process.
    let instance = unsafe { GetModuleHandleW(null()) } as HINSTANCE;
    // SAFETY: RGB values form a valid owned solid brush for process lifetime.
    let brush = unsafe { CreateSolidBrush(0x00110e0c) } as HBRUSH;
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
        hbrBackground: brush,
        ..empty_class
    };
    // SAFETY: class points to process-lifetime strings and a valid window procedure.
    if unsafe { RegisterClassW(&class) } == 0 {
        return Err("RegisterClassW failed".into());
    }

    // SAFETY: every pointer is null or points to a live zero-terminated UTF-16 string.
    let window = unsafe {
        CreateWindowExW(
            WS_EX_APPWINDOW | WS_EX_NOREDIRECTIONBITMAP,
            class_name.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            WIDTH,
            HEIGHT,
            null_mut(),
            null_mut(),
            instance,
            null_mut::<c_void>(),
        )
    };
    if window.is_null() {
        Err("CreateWindowExW failed".into())
    } else {
        Ok(window)
    }
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

fn pump_for(duration: Duration) {
    let started = Instant::now();
    while started.elapsed() < duration {
        // SAFETY: message storage is initialized and valid for the duration of each call.
        unsafe {
            let mut message: MSG = zeroed();
            while PeekMessageW(&mut message, null_mut(), 0, 0, PM_REMOVE) != 0 {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        std::thread::yield_now();
    }
}

fn main() -> Result<(), String> {
    let sources = sources();
    if sources.is_empty() {
        return Err("no eligible visible source windows".into());
    }
    let destination = create_destination()?;
    // SAFETY: destination is a valid caller-owned top-level window.
    unsafe {
        ShowWindow(destination, SW_SHOW);
    }
    pump_for(Duration::from_millis(40));

    let twenty = register_workload(destination, &sources, 20)?;
    let fifty = register_workload(destination, &sources, 50)?;

    // The production path is initiated by the hotkey and owns foreground input while the overview is open.
    // SAFETY: both handles are valid top-level windows at this point.
    let overview_foreground = unsafe { SetForegroundWindow(destination) != 0 };
    pump_for(Duration::from_millis(20));
    let activation_started = Instant::now();
    // SAFETY: the source came from EnumWindows and is still validated by SetForegroundWindow.
    let activation_call = unsafe { SetForegroundWindow(sources[0].0) != 0 };
    let activated = loop {
        // SAFETY: GetForegroundWindow has no preconditions.
        if unsafe { GetForegroundWindow() == sources[0].0 } {
            break true;
        }
        if activation_started.elapsed() >= Duration::from_millis(100) {
            break false;
        }
        pump_for(Duration::from_millis(1));
    };
    let activation_ms = activation_started.elapsed().as_secs_f64() * 1000.0;

    // SAFETY: destination is a live caller-owned top-level window.
    unsafe { DestroyWindow(destination) };

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
