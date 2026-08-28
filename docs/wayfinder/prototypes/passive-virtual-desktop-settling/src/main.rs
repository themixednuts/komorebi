use std::collections::{BTreeMap, BTreeSet};
use std::ffi::c_void;
use std::fs::File;
use std::io::BufWriter;
use std::mem::{size_of, zeroed};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use clap::{Parser, Subcommand};
use serde::Serialize;
use windows::Win32::Foundation::{CloseHandle, FILETIME, HANDLE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute};
use windows::Win32::Graphics::Gdi::UpdateWindow;
use windows::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
};
use windows::Win32::System::SystemInformation::GetTickCount64;
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentProcessId, GetProcessTimes, OpenProcess, OpenProcessToken,
    PROCESS_NAME_FORMAT, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
use windows::Win32::UI::Shell::{IVirtualDesktopManager, VirtualDesktopManager};
use windows::Win32::UI::WindowsAndMessaging::{
    CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DispatchMessageW, EVENT_OBJECT_CLOAKED,
    EVENT_OBJECT_NAMECHANGE, EVENT_OBJECT_UNCLOAKED, EVENT_SYSTEM_DESKTOPSWITCH,
    EVENT_SYSTEM_FOREGROUND, EnumWindows, GetClassNameW, GetDesktopWindow, GetForegroundWindow,
    GetMessageW, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsIconic,
    IsWindow, IsWindowVisible, MSG, PM_REMOVE, PeekMessageW, PostQuitMessage, RegisterClassW,
    SW_SHOW, SW_SHOWMINIMIZED, ShowWindow, TranslateMessage, WINDOW_EX_STYLE,
    WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS, WM_DESTROY, WNDCLASSW, WS_EX_TOOLWINDOW,
    WS_OVERLAPPEDWINDOW,
};
use windows::core::{BOOL, Error as WindowsError, HSTRING, PCWSTR, PWSTR, w};

const TARGET_WINDOW_COUNT: usize = 32;
const REQUIRED_STABLE_POLLS: u32 = 3;

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Target,
    Observe {
        #[arg(long)]
        interval_ms: u64,
        #[arg(long, default_value_t = 20)]
        transitions: usize,
        #[arg(long)]
        phase: String,
        #[arg(long)]
        output: PathBuf,
        #[arg(long, default_value_t = 600)]
        timeout_seconds: u64,
    },
    Events {
        #[arg(long, default_value_t = 30)]
        duration_seconds: u64,
        #[arg(long)]
        output: PathBuf,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
enum WindowCategory {
    ProbeNormal,
    ProbePinCandidate,
    ProbeMinimized,
    Packaged,
    Elevated,
    Ordinary,
}

#[derive(Debug, Clone)]
struct TrackedWindow {
    alias: String,
    hwnd: isize,
    process_id: u32,
    process_name: String,
    class_name: String,
    category: WindowCategory,
    elevated: Option<bool>,
}

#[derive(Debug, Serialize)]
struct WindowDescriptor {
    alias: String,
    process_id: u32,
    process_name: String,
    class_name: String,
    category: WindowCategory,
    elevated: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum ApiValue<T> {
    Ok { value: T },
    Error { hresult: i32 },
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct WindowObservation {
    exists: bool,
    visible: bool,
    minimized: bool,
    cloaked: ApiValue<bool>,
    desktop_id: ApiValue<String>,
    on_current_desktop: ApiValue<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ProbeSignature(Vec<(String, WindowObservation)>);

#[derive(Debug, Serialize)]
struct TransitionTrace {
    ordinal: usize,
    input_to_first_change_ms: u64,
    input_to_stable_ms: u64,
    first_change_to_stable_ms: u64,
    signature_changes_before_stable: u32,
    foreground_at_first_change: String,
    foreground_at_settle: String,
    probe_on_current_at_settle: usize,
    probe_desktop_ids_at_settle: Vec<String>,
}

#[derive(Debug, Default, Serialize)]
struct WindowAggregate {
    outcomes: BTreeMap<String, u64>,
}

#[derive(Debug, Serialize)]
struct RunResult {
    prototype: &'static str,
    phase: String,
    interval_ms: u64,
    requested_transitions: usize,
    completed_transitions: usize,
    stable_polls: u32,
    tracked_windows: Vec<WindowDescriptor>,
    window_aggregates: BTreeMap<String, WindowAggregate>,
    transitions: Vec<TransitionTrace>,
    poll_count: u64,
    public_query_count: u64,
    elapsed_ms: u64,
    user_cpu_ms: u64,
    kernel_cpu_ms: u64,
    timed_out: bool,
}

#[derive(Debug, Clone)]
struct CandidateState {
    signature: ProbeSignature,
    stable_polls: u32,
    first_change: Instant,
    input_marker_tick_ms: u64,
    first_change_tick_ms: u64,
    signature_changes: u32,
    foreground_at_first_change: String,
}

#[derive(Debug, Clone, Copy)]
struct ProcessTimes {
    kernel_100ns: u64,
    user_100ns: u64,
}

#[derive(Debug, Clone)]
struct RawNativeEvent {
    observed_tick_ms: u64,
    event: u32,
    hwnd: isize,
    object_id: i32,
    child_id: i32,
    event_thread_id: u32,
    event_tick_ms: u32,
}

#[derive(Debug, Serialize)]
struct NativeEvent {
    elapsed_ms: u64,
    kind: &'static str,
    hwnd: isize,
    window_alias: Option<String>,
    object_id: i32,
    child_id: i32,
    event_thread_id: u32,
    event_tick_ms: u32,
}

#[derive(Debug, Serialize)]
struct NativeEventRun {
    prototype: &'static str,
    duration_ms: u64,
    tracked_windows: Vec<WindowDescriptor>,
    event_counts: BTreeMap<&'static str, usize>,
    events: Vec<NativeEvent>,
    user_cpu_ms: u64,
    kernel_cpu_ms: u64,
}

static NATIVE_EVENTS: OnceLock<Mutex<Vec<RawNativeEvent>>> = OnceLock::new();

fn main() -> Result<(), Box<dyn std::error::Error>> {
    match Cli::parse().command {
        Command::Target => run_target()?,
        Command::Observe {
            interval_ms,
            transitions,
            phase,
            output,
            timeout_seconds,
        } => run_observer(
            interval_ms,
            transitions,
            phase,
            output,
            Duration::from_secs(timeout_seconds),
        )?,
        Command::Events {
            duration_seconds,
            output,
        } => run_native_event_probe(Duration::from_secs(duration_seconds), output)?,
    }

    Ok(())
}

fn run_native_event_probe(
    duration: Duration,
    output: PathBuf,
) -> Result<(), Box<dyn std::error::Error>> {
    let windows = enumerate_tracked_windows()?;
    if windows.iter().filter(|window| is_probe(window)).count() < 28 {
        return Err("target process is missing one or more probe windows".into());
    }

    let capture = NATIVE_EVENTS.get_or_init(|| Mutex::new(Vec::new()));
    capture
        .lock()
        .map_err(|_| "native event capture lock was poisoned")?
        .clear();

    let hooks = [
        native_event_hook(EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND)?,
        native_event_hook(EVENT_SYSTEM_DESKTOPSWITCH, EVENT_SYSTEM_DESKTOPSWITCH)?,
        native_event_hook(EVENT_OBJECT_NAMECHANGE, EVENT_OBJECT_NAMECHANGE)?,
        native_event_hook(EVENT_OBJECT_CLOAKED, EVENT_OBJECT_UNCLOAKED)?,
    ];
    let started = Instant::now();
    let started_tick_ms = unsafe { GetTickCount64() };
    let process_times_before = process_times()?;
    println!("EVENT_READY duration_ms={}", duration.as_millis());

    while started.elapsed() < duration {
        let mut message = MSG::default();
        while unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE) }.as_bool() {
            unsafe {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        thread::sleep(Duration::from_millis(1));
    }

    for hook in hooks {
        unsafe {
            let _ = UnhookWinEvent(hook);
        }
    }

    let process_times_after = process_times()?;
    let raw_events = capture
        .lock()
        .map_err(|_| "native event capture lock was poisoned")?
        .clone();
    let events = raw_events
        .into_iter()
        .map(|event| NativeEvent {
            elapsed_ms: event.observed_tick_ms.saturating_sub(started_tick_ms),
            kind: native_event_name(event.event),
            hwnd: event.hwnd,
            window_alias: if event.hwnd == unsafe { GetDesktopWindow() }.0 as isize {
                Some("desktop_window".to_string())
            } else {
                windows
                    .iter()
                    .find(|window| window.hwnd == event.hwnd)
                    .map(|window| window.alias.clone())
            },
            object_id: event.object_id,
            child_id: event.child_id,
            event_thread_id: event.event_thread_id,
            event_tick_ms: event.event_tick_ms,
        })
        .collect::<Vec<_>>();
    let event_counts = events.iter().fold(BTreeMap::new(), |mut counts, event| {
        *counts.entry(event.kind).or_default() += 1;
        counts
    });
    let result = NativeEventRun {
        prototype: "passive_virtual_desktop_native_events",
        duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        tracked_windows: windows.iter().map(WindowDescriptor::from).collect(),
        event_counts,
        events,
        user_cpu_ms: process_times_after
            .user_100ns
            .saturating_sub(process_times_before.user_100ns)
            / 10_000,
        kernel_cpu_ms: process_times_after
            .kernel_100ns
            .saturating_sub(process_times_before.kernel_100ns)
            / 10_000,
    };
    serde_json::to_writer_pretty(BufWriter::new(File::create(output)?), &result)?;
    println!("EVENT_COMPLETE counts={:?}", result.event_counts);
    Ok(())
}

fn native_event_hook(event_min: u32, event_max: u32) -> windows::core::Result<HWINEVENTHOOK> {
    let hook = unsafe {
        SetWinEventHook(
            event_min,
            event_max,
            None,
            Some(capture_native_event),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        )
    };
    if hook.0.is_null() {
        Err(WindowsError::from_thread())
    } else {
        Ok(hook)
    }
}

unsafe extern "system" fn capture_native_event(
    _hook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    object_id: i32,
    child_id: i32,
    event_thread_id: u32,
    event_tick_ms: u32,
) {
    let Some(capture) = NATIVE_EVENTS.get() else {
        return;
    };
    let Ok(mut events) = capture.lock() else {
        return;
    };
    events.push(RawNativeEvent {
        observed_tick_ms: unsafe { GetTickCount64() },
        event,
        hwnd: hwnd.0 as isize,
        object_id,
        child_id,
        event_thread_id,
        event_tick_ms,
    });
}

fn native_event_name(event: u32) -> &'static str {
    match event {
        EVENT_SYSTEM_FOREGROUND => "system_foreground",
        EVENT_SYSTEM_DESKTOPSWITCH => "system_desktop_switch",
        EVENT_OBJECT_NAMECHANGE => "object_name_changed",
        EVENT_OBJECT_CLOAKED => "object_cloaked",
        EVENT_OBJECT_UNCLOAKED => "object_uncloaked",
        _ => "unexpected",
    }
}

fn run_target() -> windows::core::Result<()> {
    let instance = unsafe { windows::Win32::System::LibraryLoader::GetModuleHandleW(None)? };
    let class_name = w!("KomorebiPassiveVirtualDesktopProbe");
    let class = WNDCLASSW {
        hInstance: instance.into(),
        lpszClassName: class_name,
        lpfnWndProc: Some(probe_window_proc),
        ..Default::default()
    };

    unsafe { RegisterClassW(&class) };

    create_target_window(class_name, "VD Probe Normal", WINDOW_EX_STYLE(0), SW_SHOW)?;
    create_target_window(class_name, "VD Probe Pin Me", WINDOW_EX_STYLE(0), SW_SHOW)?;

    for index in 1..=26 {
        create_target_window(
            class_name,
            &format!("VD Probe Minimized {index:02}"),
            WS_EX_TOOLWINDOW,
            SW_SHOWMINIMIZED,
        )?;
    }

    println!("TARGET_READY visible=2 minimized=26 pid={}", unsafe {
        GetCurrentProcessId()
    });

    let mut message = MSG::default();
    while unsafe { GetMessageW(&mut message, None, 0, 0) }.as_bool() {
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }

    Ok(())
}

unsafe extern "system" fn probe_window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_DESTROY {
        unsafe { PostQuitMessage(0) };
        return LRESULT(0);
    }

    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

fn create_target_window(
    class_name: PCWSTR,
    title: &str,
    ex_style: WINDOW_EX_STYLE,
    show_command: windows::Win32::UI::WindowsAndMessaging::SHOW_WINDOW_CMD,
) -> windows::core::Result<HWND> {
    let title = HSTRING::from(title);
    let hwnd = unsafe {
        CreateWindowExW(
            ex_style,
            class_name,
            &title,
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            560,
            320,
            None,
            None,
            None,
            None,
        )?
    };
    unsafe {
        let _ = ShowWindow(hwnd, show_command);
        let _ = UpdateWindow(hwnd);
    }
    Ok(hwnd)
}

fn run_observer(
    interval_ms: u64,
    requested_transitions: usize,
    phase: String,
    output: PathBuf,
    timeout: Duration,
) -> Result<(), Box<dyn std::error::Error>> {
    if !matches!(interval_ms, 16 | 100 | 500) {
        return Err("interval must be 16, 100, or 500 milliseconds".into());
    }

    unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok()? };
    let desktop_manager: IVirtualDesktopManager =
        unsafe { CoCreateInstance(&VirtualDesktopManager, None, CLSCTX_ALL)? };
    let windows = enumerate_tracked_windows()?;

    if windows.iter().filter(|window| is_probe(window)).count() < 28 {
        return Err("target process is missing one or more probe windows".into());
    }

    println!(
        "TRACKING count={} probe={} packaged={} elevated={}",
        windows.len(),
        windows.iter().filter(|window| is_probe(window)).count(),
        windows
            .iter()
            .filter(|window| window.category == WindowCategory::Packaged)
            .count(),
        windows
            .iter()
            .filter(|window| window.elevated == Some(true))
            .count(),
    );

    let mut aggregates = windows
        .iter()
        .map(|window| (window.alias.clone(), WindowAggregate::default()))
        .collect::<BTreeMap<_, _>>();
    let mut transitions = Vec::with_capacity(requested_transitions);
    let mut baseline: Option<ProbeSignature> = None;
    let mut candidate: Option<CandidateState> = None;
    let mut initial_candidate: Option<(ProbeSignature, u32)> = None;
    let mut poll_count = 0;
    let started = Instant::now();
    let process_times_before = process_times()?;

    while transitions.len() < requested_transitions && started.elapsed() < timeout {
        let poll_started = Instant::now();
        let observations = observe_windows(&desktop_manager, &windows);
        poll_count += 1;
        record_aggregates(&mut aggregates, &windows, &observations);

        let signature = probe_signature(&windows, &observations);
        let foreground = foreground_alias(&windows);
        let now_tick_ms = unsafe { GetTickCount64() };
        let input_tick_ms = last_input_tick_ms().unwrap_or(now_tick_ms);

        if baseline.is_none() {
            match &mut initial_candidate {
                Some((previous, count)) if *previous == signature => *count += 1,
                Some((previous, count)) => {
                    *previous = signature.clone();
                    *count = 1;
                }
                None => initial_candidate = Some((signature.clone(), 1)),
            }

            if initial_candidate
                .as_ref()
                .is_some_and(|(_, count)| *count >= REQUIRED_STABLE_POLLS)
            {
                baseline = Some(signature);
                println!(
                    "READY interval_ms={interval_ms} transitions={requested_transitions} phase={phase}"
                );
            }
        } else if baseline.as_ref().is_some_and(|value| *value == signature) {
            candidate = None;
        } else {
            match &mut candidate {
                Some(state) if state.signature == signature => state.stable_polls += 1,
                Some(state) => {
                    state.signature = signature.clone();
                    state.stable_polls = 1;
                    state.signature_changes += 1;
                }
                None => {
                    candidate = Some(CandidateState {
                        signature: signature.clone(),
                        stable_polls: 1,
                        first_change: Instant::now(),
                        input_marker_tick_ms: input_tick_ms,
                        first_change_tick_ms: now_tick_ms,
                        signature_changes: 1,
                        foreground_at_first_change: foreground.clone(),
                    });
                }
            }

            if candidate
                .as_ref()
                .is_some_and(|state| state.stable_polls >= REQUIRED_STABLE_POLLS)
            {
                let state = candidate
                    .take()
                    .expect("candidate is present after readiness check");
                let input_to_first_change_ms = state
                    .first_change_tick_ms
                    .saturating_sub(state.input_marker_tick_ms);
                let first_change_to_stable_ms =
                    u64::try_from(state.first_change.elapsed().as_millis()).unwrap_or(u64::MAX);
                let probe_observations = windows
                    .iter()
                    .zip(observations.iter())
                    .filter(|(window, _)| is_probe(window))
                    .map(|(_, observation)| observation);
                let probe_on_current_at_settle = probe_observations
                    .clone()
                    .filter(|observation| {
                        observation.on_current_desktop == ApiValue::Ok { value: true }
                    })
                    .count();
                let probe_desktop_ids_at_settle = probe_observations
                    .filter_map(|observation| match &observation.desktop_id {
                        ApiValue::Ok { value } => Some(value.clone()),
                        ApiValue::Error { .. } => None,
                    })
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect();

                transitions.push(TransitionTrace {
                    ordinal: transitions.len() + 1,
                    input_to_first_change_ms,
                    input_to_stable_ms: input_to_first_change_ms
                        .saturating_add(first_change_to_stable_ms),
                    first_change_to_stable_ms,
                    signature_changes_before_stable: state.signature_changes,
                    foreground_at_first_change: state.foreground_at_first_change,
                    foreground_at_settle: foreground,
                    probe_on_current_at_settle,
                    probe_desktop_ids_at_settle,
                });
                baseline = Some(signature);
                println!(
                    "SETTLED {}/{} input_to_stable_ms={}",
                    transitions.len(),
                    requested_transitions,
                    transitions
                        .last()
                        .expect("transition was just pushed")
                        .input_to_stable_ms,
                );
            }
        }

        let interval = Duration::from_millis(interval_ms);
        if poll_started.elapsed() < interval {
            thread::sleep(interval - poll_started.elapsed());
        }
    }

    let elapsed = started.elapsed();
    let process_times_after = process_times()?;
    let completed_transitions = transitions.len();
    let result = RunResult {
        prototype: "passive_virtual_desktop_settling",
        phase,
        interval_ms,
        requested_transitions,
        completed_transitions,
        stable_polls: REQUIRED_STABLE_POLLS,
        tracked_windows: windows.iter().map(WindowDescriptor::from).collect(),
        window_aggregates: aggregates,
        transitions,
        poll_count,
        public_query_count: poll_count
            .saturating_mul(windows.len() as u64)
            .saturating_mul(2),
        elapsed_ms: u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX),
        user_cpu_ms: process_times_after
            .user_100ns
            .saturating_sub(process_times_before.user_100ns)
            / 10_000,
        kernel_cpu_ms: process_times_after
            .kernel_100ns
            .saturating_sub(process_times_before.kernel_100ns)
            / 10_000,
        timed_out: requested_transitions != completed_transitions,
    };

    serde_json::to_writer_pretty(BufWriter::new(File::create(output)?), &result)?;
    println!(
        "COMPLETE completed={} timed_out={} polls={} queries={}",
        result.completed_transitions,
        result.timed_out,
        result.poll_count,
        result.public_query_count,
    );

    Ok(())
}

fn enumerate_tracked_windows() -> Result<Vec<TrackedWindow>, Box<dyn std::error::Error>> {
    let mut handles = Vec::<isize>::new();
    unsafe {
        EnumWindows(
            Some(collect_window),
            LPARAM((&mut handles as *mut Vec<isize>) as isize),
        )?;
    }

    let own_process = unsafe { GetCurrentProcessId() };
    let mut candidates = handles
        .into_iter()
        .filter_map(|raw| describe_window(raw, own_process))
        .collect::<Vec<_>>();

    candidates.sort_by_key(|window| {
        let priority = match window.category {
            WindowCategory::ProbeNormal => 0,
            WindowCategory::ProbePinCandidate => 1,
            WindowCategory::ProbeMinimized => 2,
            WindowCategory::Packaged => 3,
            WindowCategory::Elevated => 4,
            WindowCategory::Ordinary => 5,
        };
        (priority, window.process_id, window.hwnd)
    });

    let mut selected = Vec::with_capacity(TARGET_WINDOW_COUNT);
    for window in candidates.iter().filter(|window| is_probe(window)) {
        selected.push(window.clone());
    }
    for category in [
        WindowCategory::Packaged,
        WindowCategory::Elevated,
        WindowCategory::Ordinary,
    ] {
        for window in candidates
            .iter()
            .filter(|window| window.category == category)
        {
            if selected.len() >= TARGET_WINDOW_COUNT {
                break;
            }
            selected.push(window.clone());
            if category != WindowCategory::Ordinary {
                break;
            }
        }
    }
    candidates = selected;

    for (index, window) in candidates.iter_mut().enumerate() {
        window.alias = format!("w{:02}", index + 1);
    }

    Ok(candidates)
}

unsafe extern "system" fn collect_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let handles = unsafe { &mut *(lparam.0 as *mut Vec<isize>) };
    handles.push(hwnd.0 as isize);
    true.into()
}

fn describe_window(raw: isize, own_process: u32) -> Option<TrackedWindow> {
    let hwnd = HWND(raw as *mut c_void);
    let mut process_id = 0;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut process_id)) };
    if process_id == 0 || process_id == own_process {
        return None;
    }

    let title = window_text(hwnd);
    let class_name = window_class(hwnd);
    let visible = unsafe { IsWindowVisible(hwnd).as_bool() };
    let minimized = unsafe { IsIconic(hwnd).as_bool() };
    let process_name = process_name(process_id).unwrap_or_else(|| "unavailable".to_string());
    let elevated = process_elevated(process_id);

    let category = if title == "VD Probe Normal" {
        WindowCategory::ProbeNormal
    } else if title == "VD Probe Pin Me" {
        WindowCategory::ProbePinCandidate
    } else if title.starts_with("VD Probe Minimized ") {
        WindowCategory::ProbeMinimized
    } else if is_packaged_candidate(&process_name, &class_name) {
        WindowCategory::Packaged
    } else if elevated == Some(true) || process_name.eq_ignore_ascii_case("taskmgr.exe") {
        WindowCategory::Elevated
    } else {
        WindowCategory::Ordinary
    };

    if !matches!(category, WindowCategory::Ordinary) || visible || minimized {
        Some(TrackedWindow {
            alias: String::new(),
            hwnd: raw,
            process_id,
            process_name,
            class_name,
            category,
            elevated,
        })
    } else {
        None
    }
}

fn is_packaged_candidate(process_name: &str, class_name: &str) -> bool {
    matches!(
        process_name.to_ascii_lowercase().as_str(),
        "applicationframehost.exe" | "systemsettings.exe" | "calculatorapp.exe" | "calculator.exe"
    ) || matches!(
        class_name,
        "ApplicationFrameWindow" | "Windows.UI.Core.CoreWindow"
    )
}

fn is_probe(window: &TrackedWindow) -> bool {
    matches!(
        window.category,
        WindowCategory::ProbeNormal
            | WindowCategory::ProbePinCandidate
            | WindowCategory::ProbeMinimized
    )
}

fn observe_windows(
    desktop_manager: &IVirtualDesktopManager,
    windows: &[TrackedWindow],
) -> Vec<WindowObservation> {
    windows
        .iter()
        .map(|window| {
            let hwnd = HWND(window.hwnd as *mut c_void);
            let exists = unsafe { IsWindow(Some(hwnd)).as_bool() };
            WindowObservation {
                exists,
                visible: exists && unsafe { IsWindowVisible(hwnd).as_bool() },
                minimized: exists && unsafe { IsIconic(hwnd).as_bool() },
                cloaked: if exists {
                    cloaked(hwnd)
                } else {
                    ApiValue::Error {
                        hresult: windows::Win32::Foundation::E_HANDLE.0,
                    }
                },
                desktop_id: if exists {
                    api_value(unsafe { desktop_manager.GetWindowDesktopId(hwnd) })
                        .map(|guid| format!("{:032x}", guid.to_u128()))
                } else {
                    ApiValue::Error {
                        hresult: windows::Win32::Foundation::E_HANDLE.0,
                    }
                },
                on_current_desktop: if exists {
                    api_value(unsafe { desktop_manager.IsWindowOnCurrentVirtualDesktop(hwnd) })
                        .map(|value| value.as_bool())
                } else {
                    ApiValue::Error {
                        hresult: windows::Win32::Foundation::E_HANDLE.0,
                    }
                },
            }
        })
        .collect()
}

fn probe_signature(
    windows: &[TrackedWindow],
    observations: &[WindowObservation],
) -> ProbeSignature {
    ProbeSignature(
        windows
            .iter()
            .zip(observations)
            .filter(|(window, _)| is_probe(window))
            .map(|(window, observation)| (window.alias.clone(), observation.clone()))
            .collect(),
    )
}

fn record_aggregates(
    aggregates: &mut BTreeMap<String, WindowAggregate>,
    windows: &[TrackedWindow],
    observations: &[WindowObservation],
) {
    for (window, observation) in windows.iter().zip(observations) {
        let key = serde_json::to_string(observation).expect("observation serializes");
        *aggregates
            .get_mut(&window.alias)
            .expect("every tracked window has an aggregate")
            .outcomes
            .entry(key)
            .or_default() += 1;
    }
}

fn foreground_alias(windows: &[TrackedWindow]) -> String {
    let foreground = unsafe { GetForegroundWindow() }.0 as isize;
    windows
        .iter()
        .find(|window| window.hwnd == foreground)
        .map_or_else(|| "untracked".to_string(), |window| window.alias.clone())
}

fn last_input_tick_ms() -> windows::core::Result<u64> {
    let mut info = LASTINPUTINFO {
        cbSize: u32::try_from(size_of::<LASTINPUTINFO>()).expect("LASTINPUTINFO fits in u32"),
        ..Default::default()
    };
    if !unsafe { GetLastInputInfo(&mut info) }.as_bool() {
        return Err(WindowsError::from_thread());
    }
    let now = unsafe { GetTickCount64() };
    let low_now = now as u32;
    let age = low_now.wrapping_sub(info.dwTime);
    Ok(now.saturating_sub(u64::from(age)))
}

fn cloaked(hwnd: HWND) -> ApiValue<bool> {
    let mut value = 0u32;
    let result = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            (&mut value as *mut u32).cast(),
            u32::try_from(size_of::<u32>()).expect("u32 size fits in u32"),
        )
    };
    api_value(result).map(|()| value != 0)
}

fn api_value<T>(result: windows::core::Result<T>) -> ApiValue<T> {
    match result {
        Ok(value) => ApiValue::Ok { value },
        Err(error) => ApiValue::Error {
            hresult: error.code().0,
        },
    }
}

impl<T> ApiValue<T> {
    fn map<U>(self, map: impl FnOnce(T) -> U) -> ApiValue<U> {
        match self {
            Self::Ok { value } => ApiValue::Ok { value: map(value) },
            Self::Error { hresult } => ApiValue::Error { hresult },
        }
    }
}

impl From<&TrackedWindow> for WindowDescriptor {
    fn from(window: &TrackedWindow) -> Self {
        Self {
            alias: window.alias.clone(),
            process_id: window.process_id,
            process_name: window.process_name.clone(),
            class_name: window.class_name.clone(),
            category: window.category,
            elevated: window.elevated,
        }
    }
}

fn window_text(hwnd: HWND) -> String {
    let length = unsafe { GetWindowTextLengthW(hwnd) };
    if length <= 0 {
        return String::new();
    }
    let mut buffer = vec![0u16; usize::try_from(length).unwrap_or_default() + 1];
    let read = unsafe { GetWindowTextW(hwnd, &mut buffer) };
    String::from_utf16_lossy(&buffer[..usize::try_from(read).unwrap_or_default()])
}

fn window_class(hwnd: HWND) -> String {
    let mut buffer = [0u16; 256];
    let read = unsafe { GetClassNameW(hwnd, &mut buffer) };
    String::from_utf16_lossy(&buffer[..usize::try_from(read).unwrap_or_default()])
}

fn process_name(process_id: u32) -> Option<String> {
    let process =
        unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id) }.ok()?;
    let mut buffer = vec![0u16; 32768];
    let mut length = u32::try_from(buffer.len()).ok()?;
    let result = unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_FORMAT(0),
            PWSTR(buffer.as_mut_ptr()),
            &mut length,
        )
    };
    unsafe {
        let _ = CloseHandle(process);
    }
    result.ok()?;
    PathBuf::from(String::from_utf16_lossy(
        &buffer[..usize::try_from(length).ok()?],
    ))
    .file_name()
    .map(|name| name.to_string_lossy().into_owned())
}

fn process_elevated(process_id: u32) -> Option<bool> {
    let process =
        unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id) }.ok()?;
    let mut token = HANDLE::default();
    let opened = unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) };
    unsafe {
        let _ = CloseHandle(process);
    }
    opened.ok()?;

    let mut elevation: TOKEN_ELEVATION = unsafe { zeroed() };
    let mut returned = 0;
    let result = unsafe {
        GetTokenInformation(
            token,
            TokenElevation,
            Some((&mut elevation as *mut TOKEN_ELEVATION).cast()),
            u32::try_from(size_of::<TOKEN_ELEVATION>()).ok()?,
            &mut returned,
        )
    };
    unsafe {
        let _ = CloseHandle(token);
    }
    result.ok()?;
    Some(elevation.TokenIsElevated != 0)
}

fn process_times() -> windows::core::Result<ProcessTimes> {
    let mut creation = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    unsafe {
        GetProcessTimes(
            GetCurrentProcess(),
            &mut creation,
            &mut exit,
            &mut kernel,
            &mut user,
        )?;
    }
    Ok(ProcessTimes {
        kernel_100ns: filetime_value(kernel),
        user_100ns: filetime_value(user),
    })
}

fn filetime_value(value: FILETIME) -> u64 {
    (u64::from(value.dwHighDateTime) << 32) | u64::from(value.dwLowDateTime)
}

impl From<WindowsError> for ApiValue<()> {
    fn from(error: WindowsError) -> Self {
        Self::Error {
            hresult: error.code().0,
        }
    }
}
