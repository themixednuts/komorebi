#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    ffi::OsStr,
    mem::size_of,
    os::windows::ffi::OsStrExt as _,
    sync::atomic::{AtomicBool, Ordering},
};

use anyhow::{Result, bail};
use serde::Serialize;
use windows::{
    Win32::{
        Foundation::{CloseHandle, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM},
        Graphics::Gdi::{
            BeginPaint, EndPaint, FillRect, GetStockObject, HBRUSH, PAINTSTRUCT, WHITE_BRUSH,
        },
        System::{
            LibraryLoader::GetModuleHandleW,
            Threading::{EVENT_MODIFY_STATE, OpenEventW, SetEvent},
        },
        UI::WindowsAndMessaging::{
            CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, DestroyWindow,
            DispatchMessageW, GetMessageW, IDC_ARROW, LoadCursorW, MSG, PostQuitMessage,
            RegisterClassExW, SW_SHOW, ShowWindow, TranslateMessage, WM_DESTROY, WM_LBUTTONDOWN,
            WM_PAINT, WNDCLASSEXW, WS_EX_TOPMOST, WS_OVERLAPPEDWINDOW,
        },
    },
    core::PCWSTR,
};

#[derive(Serialize)]
struct ClickReport {
    received: bool,
    x: i16,
    y: i16,
}

static REPORT_WRITE_FAILED: AtomicBool = AtomicBool::new(false);

fn write_click_report(x: i16, y: i16) -> Result<()> {
    let Some(path) = std::env::var_os("KOMOREBI_CLICK_REPORT") else {
        return Ok(());
    };
    let report = serde_json::to_vec_pretty(&ClickReport {
        received: true,
        x,
        y,
    })?;
    std::fs::write(path, report)?;
    Ok(())
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_LBUTTONDOWN => {
            let x = lparam.0 as i16;
            let y = (lparam.0 >> 16) as i16;
            if write_click_report(x, y).is_err() {
                REPORT_WRITE_FAILED.store(true, Ordering::Release);
            }
            let _ = unsafe { DestroyWindow(hwnd) };
            LRESULT(0)
        }
        WM_PAINT => {
            let mut paint = PAINTSTRUCT::default();
            let device = unsafe { BeginPaint(hwnd, &mut paint) };
            let brush = unsafe { GetStockObject(WHITE_BRUSH) };
            let _ = unsafe { FillRect(device, &paint.rcPaint, HBRUSH(brush.0)) };
            let _ = unsafe { EndPaint(hwnd, &paint) };
            LRESULT(0)
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

fn main() -> Result<()> {
    let instance: HINSTANCE = unsafe { GetModuleHandleW(None)? }.into();
    let class_name = windows::core::w!("KomorebiDecorationInteractionTarget");
    let class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(window_proc),
        hInstance: instance,
        hCursor: unsafe { LoadCursorW(None, IDC_ARROW)? },
        lpszClassName: class_name,
        ..Default::default()
    };
    if unsafe { RegisterClassExW(&class) } == 0 {
        bail!("RegisterClassExW failed");
    }
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOPMOST,
            class_name,
            PCWSTR::null(),
            WS_OVERLAPPEDWINDOW,
            480,
            260,
            900,
            560,
            None,
            None,
            Some(instance),
            None,
        )?
    };
    let _ = unsafe { ShowWindow(hwnd, SW_SHOW) };
    signal_ready()?;
    let mut message = MSG::default();
    while unsafe { GetMessageW(&mut message, None, 0, 0) }.as_bool() {
        let _ = unsafe { TranslateMessage(&message) };
        unsafe { DispatchMessageW(&message) };
    }
    if REPORT_WRITE_FAILED.load(Ordering::Acquire) {
        bail!("failed to persist the native click report");
    }
    Ok(())
}

fn signal_ready() -> Result<()> {
    let Some(name) = std::env::var_os("KOMOREBI_READY_EVENT") else {
        return Ok(());
    };
    let name = OsStr::new(&name)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let event = unsafe { OpenEventW(EVENT_MODIFY_STATE, false, PCWSTR(name.as_ptr()))? };
    unsafe {
        SetEvent(event)?;
        CloseHandle(event)?;
    }
    Ok(())
}
