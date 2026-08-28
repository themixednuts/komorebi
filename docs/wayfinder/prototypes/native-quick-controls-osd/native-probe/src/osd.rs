use std::cell::RefCell;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde::Serialize;
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, WAIT_FAILED, WAIT_TIMEOUT};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, EVENT_OBJECT_HIDE, EVENT_OBJECT_SHOW, GetClassNameW, GetWindowTextLengthW,
    GetWindowTextW, GetWindowThreadProcessId, MSG, MWMO_INPUTAVAILABLE,
    MsgWaitForMultipleObjectsEx, OBJID_WINDOW, PM_REMOVE, PeekMessageW, QS_ALLINPUT,
    TranslateMessage, WINEVENT_OUTOFCONTEXT,
};
use windows_core::PWSTR;

use crate::native_text::NativeText;

thread_local! {
    static RAW_EVENTS: RefCell<Vec<RawWindowEvent>> = const { RefCell::new(Vec::new()) };
}

#[derive(Clone, Copy)]
struct RawWindowEvent {
    kind: u32,
    window: HWND,
    object: i32,
    child: i32,
    event_time_ms: u32,
}

#[derive(Debug, Serialize)]
pub struct WindowEvent {
    pub kind: &'static str,
    pub process_id: u32,
    pub object: i32,
    pub child: i32,
    pub event_time_ms: u32,
    pub process_image: NativeText,
    pub process_image_error: Option<String>,
    pub class: NativeText,
    pub title: NativeText,
}

pub struct WindowEventObserver(HWINEVENTHOOK);

impl WindowEventObserver {
    pub fn install() -> Result<Self> {
        RAW_EVENTS.with_borrow_mut(Vec::clear);
        // SAFETY: the callback is process-lifetime code and out-of-context delivery needs no DLL.
        let hook = unsafe {
            SetWinEventHook(
                EVENT_OBJECT_SHOW,
                EVENT_OBJECT_HIDE,
                None,
                Some(window_event),
                0,
                0,
                WINEVENT_OUTOFCONTEXT,
            )
        };
        if hook.is_invalid() {
            Err(std::io::Error::last_os_error()).context("SetWinEventHook")
        } else {
            Ok(Self(hook))
        }
    }

    pub fn clear(&self) {
        debug_assert!(!self.0.is_invalid());
        RAW_EVENTS.with_borrow_mut(Vec::clear);
    }

    pub fn collect_for(&self, duration: Duration) -> Result<Vec<WindowEvent>> {
        debug_assert!(!self.0.is_invalid());
        wait_for_window_events(duration)?;
        Ok(RAW_EVENTS.with_borrow_mut(|events| {
            std::mem::take(events)
                .into_iter()
                .map(WindowEvent::from)
                .collect()
        }))
    }
}

impl Drop for WindowEventObserver {
    fn drop(&mut self) {
        // SAFETY: this wrapper owns the hook until drop.
        if !unsafe { UnhookWinEvent(self.0) }.as_bool() {
            let error = std::io::Error::last_os_error();
            eprintln!("failed to unhook WinEvent observer: {error}");
        }
    }
}

unsafe extern "system" fn window_event(
    _hook: HWINEVENTHOOK,
    kind: u32,
    window: HWND,
    object: i32,
    child: i32,
    _event_thread: u32,
    event_time_ms: u32,
) {
    if !window.is_invalid() && object == OBJID_WINDOW.0 {
        RAW_EVENTS.with_borrow_mut(|events| {
            events.push(RawWindowEvent {
                kind,
                window,
                object,
                child,
                event_time_ms,
            });
        });
    }
}

fn wait_for_window_events(duration: Duration) -> Result<()> {
    let deadline = Instant::now() + duration;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Ok(());
        }
        let timeout_ms = u32::try_from(remaining.as_millis().clamp(1, u128::from(u32::MAX)))
            .context("bounded timeout exceeds u32")?;
        // SAFETY: no kernel handles are supplied; Windows waits for queued input or the deadline.
        let outcome = unsafe {
            MsgWaitForMultipleObjectsEx(None, timeout_ms, QS_ALLINPUT, MWMO_INPUTAVAILABLE)
        };
        if outcome == WAIT_TIMEOUT {
            return Ok(());
        }
        if outcome == WAIT_FAILED {
            return Err(std::io::Error::last_os_error()).context("MsgWaitForMultipleObjectsEx");
        }

        // SAFETY: Windows reported queued input and message storage remains live for each call.
        unsafe {
            let mut message = MSG::default();
            while PeekMessageW(&raw mut message, None, 0, 0, PM_REMOVE).as_bool() {
                let _translated = TranslateMessage(&raw const message);
                DispatchMessageW(&raw const message);
            }
        }
    }
}

impl From<RawWindowEvent> for WindowEvent {
    fn from(event: RawWindowEvent) -> Self {
        let mut process_id = 0;
        // SAFETY: the process-id pointer is writable; a stale HWND produces zero.
        unsafe { GetWindowThreadProcessId(event.window, Some(&raw mut process_id)) };
        let class_utf16 = class_name(event.window);
        let title_utf16 = window_title(event.window);
        let (process_image_utf16, process_image_error) = match process_image(process_id) {
            Ok(path) => (path, None),
            Err(error) => (Vec::new(), Some(format!("{error:#}"))),
        };

        Self {
            kind: match event.kind {
                EVENT_OBJECT_SHOW => "show",
                EVENT_OBJECT_HIDE => "hide",
                _ => "other",
            },
            process_id,
            object: event.object,
            child: event.child,
            event_time_ms: event.event_time_ms,
            process_image: NativeText::from(process_image_utf16),
            process_image_error,
            class: NativeText::from(class_utf16),
            title: NativeText::from(title_utf16),
        }
    }
}

struct OwnedHandle(HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        // SAFETY: this wrapper exclusively owns the process handle until drop.
        if let Err(error) = unsafe { CloseHandle(self.0) } {
            eprintln!("failed to close process handle: {error}");
        }
    }
}

fn process_image(process_id: u32) -> Result<Vec<u16>> {
    // SAFETY: the access mask is read-only and process_id came from Windows.
    let process = OwnedHandle(
        unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id) }
            .context("OpenProcess for image name")?,
    );
    let mut buffer = vec![0; 32_768];
    let mut length = u32::try_from(buffer.len()).context("process-image buffer exceeds u32")?;
    // SAFETY: buffer is writable for length UTF-16 units and the process handle remains live.
    unsafe {
        QueryFullProcessImageNameW(
            process.0,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &raw mut length,
        )
    }
    .context("QueryFullProcessImageNameW")?;
    let length = usize::try_from(length).context("process-image length exceeds usize")?;
    buffer.truncate(length.min(buffer.len()));
    Ok(buffer)
}

fn class_name(window: HWND) -> Vec<u16> {
    let mut buffer = vec![0; 256];
    // SAFETY: buffer is writable and window came from a WinEvent callback.
    let copied = unsafe { GetClassNameW(window, &mut buffer) };
    truncate_to_copied(buffer, copied)
}

fn window_title(window: HWND) -> Vec<u16> {
    // SAFETY: the query accepts a HWND from the WinEvent callback.
    let length = unsafe { GetWindowTextLengthW(window) };
    let Ok(length) = usize::try_from(length) else {
        return Vec::new();
    };
    let Some(capacity) = length.checked_add(1) else {
        return Vec::new();
    };
    let mut buffer = vec![0; capacity];
    // SAFETY: buffer is writable and sized for the reported text plus its terminator.
    let copied = unsafe { GetWindowTextW(window, &mut buffer) };
    truncate_to_copied(buffer, copied)
}

fn truncate_to_copied(mut buffer: Vec<u16>, copied: i32) -> Vec<u16> {
    let Ok(copied) = usize::try_from(copied) else {
        return Vec::new();
    };
    buffer.truncate(copied.min(buffer.len()));
    buffer
}
