#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    ffi::OsStr,
    mem::size_of,
    os::windows::ffi::OsStrExt as _,
    time::{Duration, Instant},
};

use anyhow::{Context as _, Result, bail};
use decoration_effect_core::AssetDigest;
use serde::Serialize;
use windows::{
    Win32::{
        Foundation::{CloseHandle, HANDLE, HINSTANCE, HMODULE, HWND, LPARAM, LRESULT, WPARAM},
        Graphics::{
            Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL_11_0},
            Direct3D11::{
                D3D11_BIND_CONSTANT_BUFFER, D3D11_BUFFER_DESC, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                D3D11_SDK_VERSION, D3D11_SUBRESOURCE_DATA, D3D11_VIEWPORT, D3D11CreateDevice,
                ID3D11Buffer, ID3D11Device, ID3D11DeviceContext, ID3D11PixelShader,
                ID3D11RenderTargetView, ID3D11Texture2D, ID3D11VertexShader,
            },
            DirectComposition::{
                DCompositionCreateDevice, IDCompositionDevice, IDCompositionTarget,
                IDCompositionVisual,
            },
            Dwm::{DWMWA_BORDER_COLOR, DwmFlush, DwmGetWindowAttribute},
            Dxgi::{
                Common::{
                    DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC,
                },
                DXGI_PRESENT, DXGI_SCALING_STRETCH, DXGI_SWAP_CHAIN_DESC1,
                DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT,
                DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL, DXGI_USAGE_RENDER_TARGET_OUTPUT, IDXGIAdapter,
                IDXGIDevice, IDXGIFactory2, IDXGIOutput6, IDXGISwapChain1, IDXGISwapChain2,
            },
            Gdi::{BeginPaint, EndPaint, PAINTSTRUCT},
        },
        System::{
            Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize},
            LibraryLoader::GetModuleHandleW,
            Threading::{EVENT_MODIFY_STATE, INFINITE, OpenEventW, SetEvent},
        },
        UI::{
            HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext},
            WindowsAndMessaging::{
                CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW,
                DispatchMessageW, HTTRANSPARENT, IDC_ARROW, LWA_ALPHA, LoadCursorW, MSG, PM_REMOVE,
                PostQuitMessage, RegisterClassExW, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOSIZE,
                SWP_NOZORDER, SetLayeredWindowAttributes, SetWindowPos, ShowWindow,
                TranslateMessage, WM_DESTROY, WM_NCCREATE, WM_NCHITTEST, WM_PAINT, WNDCLASSEXW,
                WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW,
                WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_OVERLAPPEDWINDOW, WS_POPUP,
            },
        },
    },
    core::{Interface as _, PCWSTR},
};

const WIDTH: u32 = 900;
const HEIGHT: u32 = 560;
const VS: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/decoration.vs.bin"));
const PS: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/decoration.ps.bin"));
const HLSL_DXIL_PS: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/decoration.dxil.ps.bin"));
const NAGA_DXIL_PS: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/decoration.naga.dxil.ps.bin"));
const NAGA_DXBC_PS: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/decoration.naga.dxbc.ps.bin"));

#[repr(C)]
#[derive(Clone, Copy)]
struct SceneConstants {
    resolution: [f32; 2],
    time_seconds: f32,
    border_width: f32,
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
    shader_assets: Vec<ShaderAssetReport>,
    displays: Vec<DisplayReport>,
    border_restoration: BorderRestorationReport,
    rapid_motion: bool,
}

#[derive(Clone, Serialize)]
struct ShaderAssetReport {
    route: &'static str,
    bytes: usize,
    sha256: String,
    d3d11_accepted: bool,
}

#[derive(Clone, Serialize)]
struct DisplayReport {
    device_name_wtf16: Vec<u16>,
    desktop_coordinates: [i32; 4],
    bits_per_color: u32,
    color_space: i32,
    min_luminance_nits: f32,
    max_luminance_nits: f32,
    max_full_frame_luminance_nits: f32,
}

#[derive(Clone, Serialize)]
struct BorderRestorationReport {
    readback_supported: bool,
    readback_hresult: Option<i32>,
    mutation_attempted: bool,
    exact_restoration: Option<bool>,
    decision: &'static str,
}

struct ComApartment;

impl ComApartment {
    fn initialize() -> Result<Self> {
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()? };
        Ok(Self)
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

struct Renderer {
    context: ID3D11DeviceContext,
    swap_chain: IDXGISwapChain1,
    target: ID3D11RenderTargetView,
    vertex_shader: ID3D11VertexShader,
    pixel_shader: ID3D11PixelShader,
    constants: ID3D11Buffer,
    _composition: IDCompositionDevice,
    _composition_target: IDCompositionTarget,
    _visual: IDCompositionVisual,
    shader_assets: Vec<ShaderAssetReport>,
    displays: Vec<DisplayReport>,
    border_restoration: BorderRestorationReport,
}

impl Renderer {
    fn create(hwnd: HWND, border_probe: HWND) -> Result<(Self, HANDLE)> {
        let mut device = None;
        let mut context = None;
        unsafe {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                Some(&[D3D_FEATURE_LEVEL_11_0]),
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )?;
        }
        let device: ID3D11Device = device.context("D3D11 device")?;
        let context = context.context("D3D11 immediate context")?;
        let dxgi_device: IDXGIDevice = device.cast()?;
        let adapter: IDXGIAdapter = unsafe { dxgi_device.GetAdapter()? };
        let factory: IDXGIFactory2 = unsafe { adapter.GetParent()? };
        let displays = enumerate_displays(&adapter);
        let description = DXGI_SWAP_CHAIN_DESC1 {
            Width: WIDTH,
            Height: HEIGHT,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            Stereo: false.into(),
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
            BufferCount: 2,
            Scaling: DXGI_SCALING_STRETCH,
            SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
            AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
            Flags: DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT.0 as u32,
        };
        let swap_chain =
            unsafe { factory.CreateSwapChainForComposition(&device, &description, None)? };
        let swap_chain2: IDXGISwapChain2 = swap_chain.cast()?;
        unsafe { swap_chain2.SetMaximumFrameLatency(1)? };
        let latency = unsafe { swap_chain2.GetFrameLatencyWaitableObject() };
        if latency.is_invalid() {
            bail!("DXGI did not return a frame-latency wait handle");
        }

        let composition: IDCompositionDevice = unsafe { DCompositionCreateDevice(&dxgi_device)? };
        let composition_target = unsafe { composition.CreateTargetForHwnd(hwnd, true)? };
        let visual = unsafe { composition.CreateVisual()? };
        unsafe {
            visual.SetContent(&swap_chain)?;
            composition_target.SetRoot(&visual)?;
            composition.Commit()?;
        }

        let texture: ID3D11Texture2D = unsafe { swap_chain.GetBuffer(0)? };
        let mut target = None;
        unsafe { device.CreateRenderTargetView(&texture, None, Some(&mut target))? };
        let target = target.context("D3D11 render target")?;
        let mut vertex_shader = None;
        unsafe { device.CreateVertexShader(VS, None, Some(&mut vertex_shader))? };
        let vertex_shader = vertex_shader.context("D3D11 vertex shader")?;
        let mut pixel_shader = None;
        unsafe { device.CreatePixelShader(PS, None, Some(&mut pixel_shader))? };
        let pixel_shader = pixel_shader.context("D3D11 pixel shader")?;
        let shader_assets = [
            ("hand-hlsl-fxc-dxbc", PS),
            ("hand-hlsl-dxc-dxil", HLSL_DXIL_PS),
            ("wgsl-naga-hlsl-dxc-dxil", NAGA_DXIL_PS),
            ("wgsl-naga-hlsl-fxc-dxbc", NAGA_DXBC_PS),
        ]
        .into_iter()
        .map(|(route, bytes)| {
            let mut shader = None;
            let accepted = unsafe {
                device
                    .CreatePixelShader(bytes, None, Some(&mut shader))
                    .is_ok()
            };
            ShaderAssetReport {
                route,
                bytes: bytes.len(),
                sha256: format!("{:?}", AssetDigest::of(bytes)),
                d3d11_accepted: accepted,
            }
        })
        .collect();
        let border_restoration = probe_border_restoration(border_probe);
        let initial = SceneConstants {
            resolution: [WIDTH as f32, HEIGHT as f32],
            time_seconds: 0.0,
            border_width: 6.0,
        };
        let mut constants = None;
        unsafe {
            device.CreateBuffer(
                &D3D11_BUFFER_DESC {
                    ByteWidth: size_of::<SceneConstants>() as u32,
                    Usage: Default::default(),
                    BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
                    CPUAccessFlags: 0,
                    MiscFlags: 0,
                    StructureByteStride: 0,
                },
                Some(&D3D11_SUBRESOURCE_DATA {
                    pSysMem: &initial as *const _ as _,
                    ..Default::default()
                }),
                Some(&mut constants),
            )?
        };
        let constants = constants.context("D3D11 constant buffer")?;
        Ok((
            Self {
                context,
                swap_chain,
                target,
                vertex_shader,
                pixel_shader,
                constants,
                _composition: composition,
                _composition_target: composition_target,
                _visual: visual,
                shader_assets,
                displays,
                border_restoration,
            },
            latency,
        ))
    }

    fn present(&self, elapsed: Duration) -> Result<()> {
        let constants = SceneConstants {
            resolution: [WIDTH as f32, HEIGHT as f32],
            time_seconds: elapsed.as_secs_f32(),
            border_width: 6.0,
        };
        unsafe {
            self.context
                .ClearRenderTargetView(&self.target, &[0.0, 0.0, 0.0, 0.0]);
            self.context.UpdateSubresource(
                &self.constants,
                0,
                None,
                &constants as *const _ as _,
                0,
                0,
            );
            self.context
                .OMSetRenderTargets(Some(&[Some(self.target.clone())]), None);
            self.context.RSSetViewports(Some(&[D3D11_VIEWPORT {
                Width: WIDTH as f32,
                Height: HEIGHT as f32,
                MaxDepth: 1.0,
                ..Default::default()
            }]));
            self.context.VSSetShader(&self.vertex_shader, None);
            self.context.PSSetShader(&self.pixel_shader, None);
            self.context
                .VSSetConstantBuffers(0, Some(&[Some(self.constants.clone())]));
            self.context
                .PSSetConstantBuffers(0, Some(&[Some(self.constants.clone())]));
            self.context.IASetPrimitiveTopology(
                windows::Win32::Graphics::Direct3D::D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST,
            );
            self.context.Draw(3, 0);
            self.swap_chain.Present(1, DXGI_PRESENT(0)).ok()?;
        }
        Ok(())
    }
}

fn enumerate_displays(adapter: &IDXGIAdapter) -> Vec<DisplayReport> {
    let mut displays = Vec::new();
    for index in 0.. {
        let Ok(output) = (unsafe { adapter.EnumOutputs(index) }) else {
            break;
        };
        let Ok(output) = output.cast::<IDXGIOutput6>() else {
            continue;
        };
        let Ok(description) = (unsafe { output.GetDesc1() }) else {
            continue;
        };
        let name_length = description
            .DeviceName
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(description.DeviceName.len());
        displays.push(DisplayReport {
            device_name_wtf16: description.DeviceName[..name_length].to_vec(),
            desktop_coordinates: [
                description.DesktopCoordinates.left,
                description.DesktopCoordinates.top,
                description.DesktopCoordinates.right,
                description.DesktopCoordinates.bottom,
            ],
            bits_per_color: description.BitsPerColor,
            color_space: description.ColorSpace.0,
            min_luminance_nits: description.MinLuminance,
            max_luminance_nits: description.MaxLuminance,
            max_full_frame_luminance_nits: description.MaxFullFrameLuminance,
        });
    }
    displays
}

fn read_border_color(hwnd: HWND) -> windows::core::Result<u32> {
    let mut color = 0u32;
    unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_BORDER_COLOR,
            (&mut color as *mut u32).cast(),
            size_of::<u32>() as u32,
        )?;
    }
    Ok(color)
}

fn probe_border_restoration(hwnd: HWND) -> BorderRestorationReport {
    match read_border_color(hwnd) {
        Ok(_) => BorderRestorationReport {
            readback_supported: true,
            readback_hresult: None,
            mutation_attempted: false,
            exact_restoration: None,
            decision: "readable probe baseline, but foreign mutation remains disabled",
        },
        Err(error) => BorderRestorationReport {
            readback_supported: false,
            readback_hresult: Some(error.code().0),
            mutation_attempted: false,
            exact_restoration: None,
            decision: "foreign border mutation disabled because the baseline is unreadable",
        },
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_NCCREATE => {
            let create = unsafe { &*(lparam.0 as *const CREATESTRUCTW) };
            if create.lpCreateParams.is_null() {
                return LRESULT(0);
            }
            LRESULT(1)
        }
        WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
        WM_PAINT => {
            let mut paint = PAINTSTRUCT::default();
            unsafe {
                BeginPaint(hwnd, &mut paint);
                let _ = EndPaint(hwnd, &paint);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

fn create_windows() -> Result<(HWND, HWND)> {
    unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)? };
    let instance: HINSTANCE = unsafe { GetModuleHandleW(None)? }.into();
    let class_name = windows::core::w!("KomorebiDcompDecorationProbe");
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
    let marker = 1u8;
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOPMOST
                | WS_EX_TRANSPARENT
                | WS_EX_NOACTIVATE
                | WS_EX_TOOLWINDOW
                | WS_EX_NOREDIRECTIONBITMAP
                | WS_EX_LAYERED,
            class_name,
            PCWSTR::null(),
            WS_POPUP,
            480,
            260,
            WIDTH as i32,
            HEIGHT as i32,
            None,
            None,
            Some(instance),
            Some(&marker as *const _ as _),
        )?
    };
    unsafe { SetLayeredWindowAttributes(hwnd, Default::default(), 255, LWA_ALPHA)? };
    let _ = unsafe { ShowWindow(hwnd, SW_SHOWNOACTIVATE) };
    let border_probe = unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class_name,
            PCWSTR::null(),
            WS_OVERLAPPEDWINDOW,
            -32_000,
            -32_000,
            160,
            120,
            None,
            None,
            Some(instance),
            Some(&marker as *const _ as _),
        )?
    };
    let _ = unsafe { ShowWindow(border_probe, SW_SHOWNOACTIVATE) };
    unsafe { DwmFlush()? };
    Ok((hwnd, border_probe))
}

fn percentile_99(samples: &mut [f64]) -> f64 {
    samples.sort_by(f64::total_cmp);
    let index = ((samples.len().saturating_sub(1)) as f64 * 0.99).round() as usize;
    samples.get(index).copied().unwrap_or_default()
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

fn main() -> Result<()> {
    let _com = ComApartment::initialize()?;
    let (hwnd, border_probe) = create_windows()?;
    let (renderer, latency) = Renderer::create(hwnd, border_probe)?;
    signal_ready()?;
    let started = Instant::now();
    let mut previous = started;
    let mut intervals = Vec::with_capacity(1_000);
    let mut frames = 0u64;
    let rapid_motion = std::env::var_os("KOMOREBI_RAPID_MOTION").is_some();

    while started.elapsed() < Duration::from_secs(8) {
        let mut message = MSG::default();
        while unsafe {
            windows::Win32::UI::WindowsAndMessaging::PeekMessageW(
                &mut message,
                None,
                0,
                0,
                PM_REMOVE,
            )
        }
        .as_bool()
        {
            if message.message == windows::Win32::UI::WindowsAndMessaging::WM_QUIT {
                break;
            }
            unsafe {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        let wait =
            unsafe { windows::Win32::System::Threading::WaitForSingleObject(latency, INFINITE) };
        if wait != windows::Win32::Foundation::WAIT_OBJECT_0 {
            bail!("DXGI frame-latency wait failed: {wait:?}");
        }
        let now = Instant::now();
        if frames > 0 {
            intervals.push(now.duration_since(previous).as_secs_f64() * 1_000.0);
        }
        if rapid_motion {
            let phase = frames as f32 * 0.08;
            let x = 480 + (phase.sin() * 240.0) as i32;
            unsafe {
                SetWindowPos(
                    hwnd,
                    None,
                    x,
                    260,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOZORDER,
                )?;
            }
        }
        renderer.present(started.elapsed())?;
        previous = now;
        frames += 1;
    }
    unsafe { CloseHandle(latency)? };
    let elapsed_ms = started.elapsed().as_secs_f64() * 1_000.0;
    let mean = intervals.iter().sum::<f64>() / intervals.len().max(1) as f64;
    let p99 = percentile_99(&mut intervals);
    let report = serde_json::to_vec_pretty(&Report {
        backend: "dedicated-d3d11-directcomposition",
        frames,
        elapsed_ms,
        mean_frame_ms: mean,
        p99_frame_ms: p99,
        wake_source: "IDXGISwapChain2 frame-latency wait handle",
        input_inert: true,
        shader_assets: renderer.shader_assets.clone(),
        displays: renderer.displays.clone(),
        border_restoration: renderer.border_restoration.clone(),
        rapid_motion,
    })?;
    if let Some(path) = std::env::var_os("KOMOREBI_PROBE_REPORT") {
        std::fs::write(path, &report)?;
    } else {
        println!("{}", String::from_utf8(report)?);
    }
    Ok(())
}
