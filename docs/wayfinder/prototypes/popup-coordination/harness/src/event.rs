use std::array;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicIsize;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::thread;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use crossbeam_queue::ArrayQueue;
use serde::Serialize;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Foundation::HWND;
use windows::Win32::Foundation::WAIT_FAILED;
use windows::Win32::Foundation::WAIT_OBJECT_0;
use windows::Win32::System::Performance::QueryPerformanceCounter;
use windows::Win32::System::Performance::QueryPerformanceFrequency;
use windows::Win32::System::Threading::CreateEventW;
use windows::Win32::System::Threading::INFINITE;
use windows::Win32::System::Threading::SetEvent;
use windows::Win32::UI::Accessibility::HWINEVENTHOOK;
use windows::Win32::UI::Accessibility::SetWinEventHook;
use windows::Win32::UI::Accessibility::UnhookWinEvent;
use windows::Win32::UI::WindowsAndMessaging::DispatchMessageW;
use windows::Win32::UI::WindowsAndMessaging::EVENT_MAX;
use windows::Win32::UI::WindowsAndMessaging::EVENT_MIN;
use windows::Win32::UI::WindowsAndMessaging::MSG;
use windows::Win32::UI::WindowsAndMessaging::MWMO_INPUTAVAILABLE;
use windows::Win32::UI::WindowsAndMessaging::MsgWaitForMultipleObjectsEx;
use windows::Win32::UI::WindowsAndMessaging::PM_REMOVE;
use windows::Win32::UI::WindowsAndMessaging::PeekMessageW;
use windows::Win32::UI::WindowsAndMessaging::QS_ALLINPUT;
use windows::Win32::UI::WindowsAndMessaging::TranslateMessage;
use windows::Win32::UI::WindowsAndMessaging::WINEVENT_OUTOFCONTEXT;
use windows::Win32::UI::WindowsAndMessaging::WINEVENT_SKIPOWNPROCESS;
use windows::core::PCWSTR;

const HISTOGRAM_BUCKETS: usize = 256;
const HISTOGRAM_STEP_NS: u64 = 100;

#[derive(Clone, Copy, Debug, Serialize)]
pub struct RawWinEvent {
    pub sequence: u64,
    pub event: u32,
    pub window: isize,
    pub object: i32,
    pub child: i32,
    pub source_thread: u32,
    pub source_time_ms: u32,
}

#[derive(Clone, Debug, Serialize)]
pub struct CallbackMetrics {
    pub delivered: u64,
    pub dropped: u64,
    pub maximum_ns: u64,
    pub p50_ns: u64,
    pub p95_ns: u64,
    pub p99_ns: u64,
}

struct HookState {
    active: AtomicBool,
    frequency: u64,
    queue: ArrayQueue<RawWinEvent>,
    next_sequence: AtomicU64,
    delivered: AtomicU64,
    dropped: AtomicU64,
    maximum_ticks: AtomicU64,
    histogram: [AtomicU64; HISTOGRAM_BUCKETS],
    watch_window: AtomicIsize,
    watch_event: AtomicU32,
    watch_generation: AtomicU64,
    watch_notify: tokio::sync::Notify,
}

impl HookState {
    fn new(queue_capacity: usize, frequency: u64) -> Self {
        Self {
            active: AtomicBool::new(true),
            frequency,
            queue: ArrayQueue::new(queue_capacity),
            next_sequence: AtomicU64::new(0),
            delivered: AtomicU64::new(0),
            dropped: AtomicU64::new(0),
            maximum_ticks: AtomicU64::new(0),
            histogram: array::from_fn(|_| AtomicU64::new(0)),
            watch_window: AtomicIsize::new(0),
            watch_event: AtomicU32::new(0),
            watch_generation: AtomicU64::new(0),
            watch_notify: tokio::sync::Notify::new(),
        }
    }

    fn record_duration(&self, ticks: u64) {
        self.maximum_ticks.fetch_max(ticks, Ordering::Relaxed);
        let nanoseconds = ticks.saturating_mul(1_000_000_000) / self.frequency;
        let bucket = usize::try_from(nanoseconds / HISTOGRAM_STEP_NS)
            .unwrap_or(usize::MAX)
            .min(HISTOGRAM_BUCKETS - 1);
        self.histogram[bucket].fetch_add(1, Ordering::Relaxed);
    }

    fn metrics(&self) -> CallbackMetrics {
        let delivered = self.delivered.load(Ordering::Relaxed);
        CallbackMetrics {
            delivered,
            dropped: self.dropped.load(Ordering::Relaxed),
            maximum_ns: ticks_to_ns(self.maximum_ticks.load(Ordering::Relaxed), self.frequency),
            p50_ns: self.quantile_ns(delivered, 50),
            p95_ns: self.quantile_ns(delivered, 95),
            p99_ns: self.quantile_ns(delivered, 99),
        }
    }

    fn quantile_ns(&self, count: u64, percentile: u64) -> u64 {
        if count == 0 {
            return 0;
        }
        let target = count.saturating_mul(percentile).div_ceil(100);
        let mut seen = 0_u64;
        for (bucket, value) in self.histogram.iter().enumerate() {
            seen = seen.saturating_add(value.load(Ordering::Relaxed));
            if seen >= target {
                return u64::try_from(bucket)
                    .unwrap_or(u64::MAX)
                    .saturating_mul(HISTOGRAM_STEP_NS);
            }
        }
        u64::try_from(HISTOGRAM_BUCKETS - 1)
            .unwrap_or(u64::MAX)
            .saturating_mul(HISTOGRAM_STEP_NS)
    }
}

static CALLBACK_STATE: OnceLock<Arc<HookState>> = OnceLock::new();

unsafe extern "system" fn callback(
    _hook: HWINEVENTHOOK,
    event: u32,
    window: HWND,
    object: i32,
    child: i32,
    source_thread: u32,
    source_time_ms: u32,
) {
    let start = performance_counter();
    let Some(state) = CALLBACK_STATE.get() else {
        return;
    };
    if !state.active.load(Ordering::Relaxed) || window.0.is_null() {
        return;
    }
    let sequence = state.next_sequence.fetch_add(1, Ordering::Relaxed);
    let item = RawWinEvent {
        sequence,
        event,
        window: isize::try_from(window.0.addr()).unwrap_or(isize::MAX),
        object,
        child,
        source_thread,
        source_time_ms,
    };
    state.delivered.fetch_add(1, Ordering::Relaxed);
    if state.queue.push(item).is_err() {
        state.dropped.fetch_add(1, Ordering::Relaxed);
    }
    let is_watched = state.watch_window.load(Ordering::Acquire) == item.window
        && state.watch_event.load(Ordering::Acquire) == event;
    if is_watched {
        state.watch_generation.fetch_add(1, Ordering::Release);
        state.watch_notify.notify_one();
    }
    state.record_duration(performance_counter().saturating_sub(start));
}

pub struct EventObserver {
    state: Arc<HookState>,
    hook_thread: Option<thread::JoinHandle<Result<()>>>,
    stop_event: isize,
}

impl EventObserver {
    pub async fn install(process_id: u32, queue_capacity: usize) -> Result<Self> {
        let frequency = performance_frequency()?;
        let state = Arc::new(HookState::new(queue_capacity, frequency));
        CALLBACK_STATE
            .set(Arc::clone(&state))
            .map_err(|_| anyhow::anyhow!("only one event observer may be installed per process"))?;
        // SAFETY: Null security attributes and name request a private manual-reset event.
        let stop_event = unsafe { CreateEventW(None, true, false, PCWSTR::null()) }?;
        let stop_raw =
            isize::try_from(stop_event.0.addr()).context("stop event address overflow")?;
        let (ready_sender, ready_receiver) = tokio::sync::oneshot::channel();
        let hook_thread = thread::Builder::new()
            .name("popup-winevent-hook".to_owned())
            .spawn(move || hook_loop(process_id, stop_raw, ready_sender));
        let hook_thread = match hook_thread {
            Ok(thread) => thread,
            Err(error) => {
                // SAFETY: Thread creation failed, so this branch uniquely owns the event handle.
                unsafe { CloseHandle(stop_event) }?;
                return Err(error.into());
            }
        };
        let mut pending = PendingHook {
            stop_event: stop_raw,
            hook_thread: Some(hook_thread),
            armed: true,
        };
        ready_receiver
            .await
            .context("WinEvent hook thread exited before readiness")??;
        pending.armed = false;
        Ok(Self {
            state,
            hook_thread: pending.hook_thread.take(),
            stop_event: stop_raw,
        })
    }

    pub fn drain(&self) -> Vec<RawWinEvent> {
        let mut events = Vec::with_capacity(self.state.queue.len());
        while let Some(event) = self.state.queue.pop() {
            events.push(event);
        }
        events
    }

    pub fn metrics(&self) -> CallbackMetrics {
        self.state.metrics()
    }

    pub fn arm_watch(&self, window: isize, event: u32) -> u64 {
        self.state.watch_window.store(window, Ordering::Release);
        self.state.watch_event.store(event, Ordering::Release);
        self.state.watch_generation.load(Ordering::Acquire)
    }

    pub async fn wait_for_watch(&self, after: u64) {
        loop {
            let notified = self.state.watch_notify.notified();
            if self.state.watch_generation.load(Ordering::Acquire) > after {
                return;
            }
            notified.await;
        }
    }

    pub async fn stop(mut self) -> Result<(Vec<RawWinEvent>, CallbackMetrics)> {
        self.state.active.store(false, Ordering::Release);
        signal_event(self.stop_event)?;
        let Some(handle) = self.hook_thread.take() else {
            bail!("event hook thread was already joined");
        };
        tokio::task::spawn_blocking(move || handle.join())
            .await
            .context("event hook join task was cancelled")?
            .map_err(|_| anyhow::anyhow!("event hook thread panicked"))??;
        Ok((self.drain(), self.metrics()))
    }
}

impl Drop for EventObserver {
    fn drop(&mut self) {
        self.state.active.store(false, Ordering::Release);
        // Drop cannot report shutdown failure; the process also owns no persistent hook state.
        let _ = signal_event(self.stop_event);
    }
}

struct PendingHook {
    stop_event: isize,
    hook_thread: Option<thread::JoinHandle<Result<()>>>,
    armed: bool,
}

impl Drop for PendingHook {
    fn drop(&mut self) {
        if self.armed {
            // Cancellation before readiness has no caller to receive an error; signaling still
            // makes the hook thread converge and close its handles.
            let _ = signal_event(self.stop_event);
        }
    }
}

fn hook_loop(
    process_id: u32,
    stop_raw: isize,
    ready: tokio::sync::oneshot::Sender<Result<()>>,
) -> Result<()> {
    let stop_event = handle_from_raw(stop_raw);
    // SAFETY: The callback has the required ABI and remains linked for the hook lifetime; this
    // thread owns the required message pump and targets only the supplied process.
    let hook = unsafe {
        SetWinEventHook(
            EVENT_MIN,
            EVENT_MAX,
            None,
            Some(callback),
            process_id,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        )
    };
    if hook.0.is_null() {
        let error = windows::core::Error::from_thread();
        // Receiver closure only means installation was cancelled; no error consumer remains.
        let _ = ready.send(Err(error.clone().into()));
        // SAFETY: No hook was installed and this thread uniquely owns the event handle.
        unsafe { CloseHandle(stop_event) }?;
        bail!("SetWinEventHook failed: {error}");
    }
    if ready.send(Ok(())).is_err() {
        // SAFETY: This thread owns both handles and no callback state will be consumed by a caller.
        let unhooked = unsafe { UnhookWinEvent(hook) };
        // SAFETY: The cancelled installation left this thread as the unique event-handle owner.
        unsafe { CloseHandle(stop_event) }?;
        if !unhooked.as_bool() {
            return Err(windows::core::Error::from_thread().into());
        }
        return Ok(());
    }
    let mut message = MSG::default();
    let mut wait_error = None;
    'hook: loop {
        // SAFETY: `stop_event` remains open on this thread and the handle slice lives for the call.
        let status = unsafe {
            MsgWaitForMultipleObjectsEx(
                Some(&[stop_event]),
                INFINITE,
                QS_ALLINPUT,
                MWMO_INPUTAVAILABLE,
            )
        };
        if status == WAIT_OBJECT_0 {
            break;
        }
        if status == WAIT_FAILED {
            wait_error = Some(windows::core::Error::from_thread());
            break;
        }
        // SAFETY: `message` is valid writable storage; removed messages are dispatched here.
        while unsafe { PeekMessageW(&raw mut message, None, 0, 0, PM_REMOVE) }.as_bool() {
            if message.message == windows::Win32::UI::WindowsAndMessaging::WM_QUIT {
                break 'hook;
            }
            // SAFETY: `message` was initialized by PeekMessageW and remains alive for both calls.
            unsafe {
                // A false result only means this message needs no keyboard translation.
                let _ = TranslateMessage(&raw const message);
                DispatchMessageW(&raw const message);
            }
        }
    }
    // SAFETY: `hook` is valid and uniquely unhooked on its installing thread.
    let unhooked = unsafe { UnhookWinEvent(hook) };
    // SAFETY: The message loop has stopped and this thread uniquely owns the event handle.
    unsafe { CloseHandle(stop_event) }?;
    if let Some(error) = wait_error {
        return Err(error.into());
    }
    if !unhooked.as_bool() {
        return Err(windows::core::Error::from_thread().into());
    }
    Ok(())
}

fn signal_event(raw: isize) -> Result<()> {
    // SAFETY: `raw` originates from the live stop event and stays open until the hook thread exits.
    unsafe { SetEvent(handle_from_raw(raw)) }?;
    Ok(())
}

fn handle_from_raw(raw: isize) -> HANDLE {
    let address = usize::try_from(raw).unwrap_or(usize::MAX);
    HANDLE(std::ptr::with_exposed_provenance_mut::<std::ffi::c_void>(
        address,
    ))
}

fn performance_frequency() -> Result<u64> {
    let mut value = 0_i64;
    // SAFETY: `value` is valid writable storage for the duration of the call.
    unsafe { QueryPerformanceFrequency(&raw mut value) }?;
    u64::try_from(value).context("performance counter frequency must be positive")
}

fn performance_counter() -> u64 {
    let mut value = 0_i64;
    // SAFETY: `value` is valid writable storage for the duration of the call.
    let succeeded = unsafe { QueryPerformanceCounter(&raw mut value) }.is_ok();
    if !succeeded {
        return 0;
    }
    u64::try_from(value).unwrap_or(0)
}

fn ticks_to_ns(ticks: u64, frequency: u64) -> u64 {
    ticks.saturating_mul(1_000_000_000) / frequency
}
