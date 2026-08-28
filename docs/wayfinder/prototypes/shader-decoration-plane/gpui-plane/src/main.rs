#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    ffi::OsStr,
    os::windows::ffi::OsStrExt as _,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use decoration_effect_core::{
    BorderParameters, EffectBudget, EffectId, EffectInstance, EffectLifetime, EffectParameters,
    Generation, Rgba, SemanticTarget,
};
use gpui::{
    App, AppContext as _, Bounds, Context, IntoElement, ParentElement as _, Render, Styled as _,
    Window, WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions, canvas, div,
    point, px, rgba, size,
};
use raw_window_handle::RawWindowHandle;
use serde::Serialize;
use windows::{
    Win32::{
        Foundation::{CloseHandle, HWND},
        System::{
            LibraryLoader::{GetModuleHandleA, GetProcAddress},
            Threading::{EVENT_MODIFY_STATE, OpenEventW, SetEvent},
        },
        UI::WindowsAndMessaging::{
            GWL_EXSTYLE, GetWindowLongPtrW, LWA_ALPHA, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER,
            SetLayeredWindowAttributes, SetWindowLongPtrW, SetWindowPos, WS_EX_LAYERED,
            WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT,
        },
    },
    core::{PCSTR, PCWSTR},
};

struct DecorationView {
    effect: EffectInstance,
    started_at: Instant,
    metrics: Arc<Mutex<FrameMetrics>>,
}

impl DecorationView {
    fn new(effect: EffectInstance, metrics: Arc<Mutex<FrameMetrics>>) -> Self {
        Self {
            effect,
            started_at: Instant::now(),
            metrics,
        }
    }
}

impl Render for DecorationView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let EffectParameters::FocusBorder(border) = self.effect.parameters else {
            return div().size_full();
        };
        let elapsed = self.started_at.elapsed().as_secs_f32();
        let pulse = (elapsed * border.pulse_hz * std::f32::consts::TAU).sin() * 0.12 + 0.84;
        let color = border.color;
        let metrics = Arc::clone(&self.metrics);
        let rapid_motion = std::env::var_os("KOMOREBI_RAPID_MOTION").is_some();

        div().size_full().child(canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                let frame = metrics
                    .lock()
                    .map(|mut metrics| metrics.record())
                    .unwrap_or_default();
                if rapid_motion {
                    move_surface(window, frame);
                }
                let alpha = (color.alpha * pulse).clamp(0.0, 1.0);
                let stroke = rgba(
                    ((color.red * 255.0) as u32) << 24
                        | ((color.green * 255.0) as u32) << 16
                        | ((color.blue * 255.0) as u32) << 8
                        | (alpha * 255.0) as u32,
                );
                for offset in 0..border.width_px as u32 {
                    let inset = px(offset as f32);
                    let outline = Bounds {
                        origin: bounds.origin + point(inset, inset),
                        size: size(
                            bounds.size.width - inset * 2.0,
                            bounds.size.height - inset * 2.0,
                        ),
                    };
                    window.paint_quad(gpui::outline(outline, stroke, gpui::BorderStyle::default()));
                }
                window.request_animation_frame();
            },
        ))
    }
}

#[derive(Default)]
struct FrameMetrics {
    previous: Option<Instant>,
    intervals_ms: Vec<f64>,
    frames: u64,
}

impl FrameMetrics {
    fn record(&mut self) -> u64 {
        let now = Instant::now();
        if let Some(previous) = self.previous.replace(now) {
            self.intervals_ms
                .push(now.duration_since(previous).as_secs_f64() * 1_000.0);
        }
        self.frames += 1;
        self.frames
    }

    fn report(&mut self, elapsed: Duration) -> Report {
        self.intervals_ms.sort_by(f64::total_cmp);
        let index = ((self.intervals_ms.len().saturating_sub(1)) as f64 * 0.99).round() as usize;
        let mean = self.intervals_ms.iter().sum::<f64>() / self.intervals_ms.len().max(1) as f64;
        Report {
            backend: "gpui-canvas-existing-scene",
            frames: self.frames,
            elapsed_ms: elapsed.as_secs_f64() * 1_000.0,
            mean_frame_ms: mean,
            p99_frame_ms: self.intervals_ms.get(index).copied().unwrap_or_default(),
            wake_source: "GPUI request_animation_frame",
            input_inert: true,
            rapid_motion: std::env::var_os("KOMOREBI_RAPID_MOTION").is_some(),
        }
    }
}

#[derive(Serialize)]
struct Report {
    backend: &'static str,
    frames: u64,
    elapsed_ms: f64,
    mean_frame_ms: f64,
    p99_frame_ms: f64,
    wake_source: &'static str,
    input_inert: bool,
    rapid_motion: bool,
}

fn move_surface(window: &Window, frame: u64) {
    let Ok(handle) = raw_window_handle::HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return;
    };
    let phase = frame as f32 * 0.08;
    let x = 480 + (phase.sin() * 240.0) as i32;
    let hwnd = HWND(handle.hwnd.get() as *mut _);
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            None,
            x,
            260,
            0,
            0,
            SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOZORDER,
        );
    }
}

#[repr(C)]
struct AccentPolicy {
    state: u32,
    flags: u32,
    color: u32,
    animation_id: u32,
}

#[repr(C)]
struct WindowCompositionAttributeData {
    attribute: u32,
    data: *mut std::ffi::c_void,
    data_size: usize,
}

fn make_input_inert(window: &Window) {
    let Ok(handle) = raw_window_handle::HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return;
    };
    let hwnd = HWND(handle.hwnd.get() as *mut _);
    unsafe {
        let current = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let additions =
            (WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_LAYERED).0 as isize;
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, current | additions);
        let _ = SetLayeredWindowAttributes(hwnd, Default::default(), 255, LWA_ALPHA);
    }
    disable_gpui_accent_tint(hwnd);
    signal_ready();
}

fn signal_ready() {
    let Some(name) = std::env::var_os("KOMOREBI_READY_EVENT") else {
        return;
    };
    let name = OsStr::new(&name)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let Ok(event) = (unsafe { OpenEventW(EVENT_MODIFY_STATE, false, PCWSTR(name.as_ptr())) })
    else {
        return;
    };
    unsafe {
        let _ = SetEvent(event);
        let _ = CloseHandle(event);
    }
}

fn disable_gpui_accent_tint(hwnd: HWND) {
    type SetWindowCompositionAttribute =
        unsafe extern "system" fn(HWND, *mut WindowCompositionAttributeData) -> i32;

    let Ok(user32) = (unsafe { GetModuleHandleA(PCSTR(c"user32.dll".as_ptr().cast())) }) else {
        return;
    };
    let Some(function) = (unsafe {
        GetProcAddress(
            user32,
            PCSTR(c"SetWindowCompositionAttribute".as_ptr().cast()),
        )
    }) else {
        return;
    };
    // The symbol and ABI are the same private Windows seam already used by pinned GPUI.
    let set_attribute: SetWindowCompositionAttribute = unsafe { std::mem::transmute(function) };
    let mut accent = AccentPolicy {
        state: 0,
        flags: 2,
        color: 0,
        animation_id: 0,
    };
    let mut data = WindowCompositionAttributeData {
        attribute: 0x13,
        data: (&mut accent as *mut AccentPolicy).cast(),
        data_size: std::mem::size_of::<AccentPolicy>(),
    };
    // Both pointers remain valid for the duration of the synchronous call.
    let _ = unsafe { set_attribute(hwnd, &mut data) };
}

fn prototype_effect() -> anyhow::Result<EffectInstance> {
    Ok(EffectInstance {
        id: EffectId::checked(1)?,
        generation: Generation::INITIAL,
        target: SemanticTarget::FocusedWindowOutline,
        parameters: EffectParameters::FocusBorder(BorderParameters::checked(
            6.0,
            18.0,
            Rgba::checked(1.0, 0.16, 0.55, 0.92)?,
            0.8,
        )?),
        lifetime: EffectLifetime::fixed(Duration::from_secs(10))?,
        budget: EffectBudget::checked(96, 0)?,
    })
}

fn write_report(metrics: &Arc<Mutex<FrameMetrics>>, started: Instant) -> anyhow::Result<()> {
    let report = metrics
        .lock()
        .map_err(|_| anyhow::anyhow!("frame metrics lock was poisoned"))?
        .report(started.elapsed());
    let bytes = serde_json::to_vec_pretty(&report)?;
    if let Some(path) = std::env::var_os("KOMOREBI_PROBE_REPORT") {
        std::fs::write(path, bytes)?;
    }
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let effect = prototype_effect()?;
    gpui_platform::application().run(move |cx: &mut App| {
        let metrics = Arc::new(Mutex::new(FrameMetrics::default()));
        let started = Instant::now();
        let bounds = WindowBounds::Windowed(Bounds {
            origin: point(px(480.0), px(260.0)),
            size: size(px(900.0), px(560.0)),
        });
        let result = cx.open_window(
            WindowOptions {
                window_bounds: Some(bounds),
                titlebar: None,
                focus: false,
                kind: WindowKind::PopUp,
                is_movable: false,
                is_resizable: false,
                is_minimizable: false,
                inactive_frame_interval: None,
                window_background: WindowBackgroundAppearance::Transparent,
                ..Default::default()
            },
            {
                let metrics = Arc::clone(&metrics);
                let effect = effect.clone();
                move |window, cx| {
                    make_input_inert(window);
                    cx.new(|_| DecorationView::new(effect, metrics))
                }
            },
        );
        if let Err(error) = result {
            eprintln!("open GPUI decoration surface: {error}");
            cx.quit();
            return;
        }

        let timer = cx.background_executor().timer(Duration::from_secs(8));
        cx.spawn(async move |cx| {
            timer.await;
            if let Err(error) = write_report(&metrics, started) {
                eprintln!("write GPUI probe report: {error:#}");
            }
            cx.update(|cx| cx.quit());
        })
        .detach();
    });
    Ok(())
}
