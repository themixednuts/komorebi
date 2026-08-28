use std::mem::size_of;
use std::ptr;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, bail};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, TRUE, WPARAM};
use windows_sys::Win32::Graphics::{Dwm as dwm, Gdi as gdi};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::GetCurrentProcessId;
use windows_sys::Win32::UI::WindowsAndMessaging as wm;

use crate::model::{Scenario, SmokeReport};
use crate::native;

const COVER_COLOR: u32 = 0x0024_160f;
const PLACEHOLDER_COLOR: u32 = 0x0050_4540;

pub fn smoke(window_count: usize, live_limit: usize, scenario: Scenario) -> Result<()> {
    let report = measure_once(window_count, live_limit, scenario)?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

pub fn measure_once(
    window_count: usize,
    live_limit: usize,
    scenario: Scenario,
) -> Result<SmokeReport> {
    if !matches!(window_count, 20 | 50) {
        bail!("window count must be 20 or 50")
    }

    let class = WindowClass::register()?;
    let work = native::work_area()?;
    let cover = class.create_cover(work)?;
    let helpers = class.create_helpers(window_count, work)?;
    let external = native::visible_windows()?
        .into_iter()
        .filter(|window| {
            window.process_id != unsafe { GetCurrentProcessId() }
                && window.class_display != "Progman"
                && !window.class_display.starts_with("komoborder-")
                && !window.title_display.is_empty()
        })
        .take(window_count / 2)
        .collect::<Vec<_>>();
    let external_source_classes = external
        .iter()
        .map(|window| window.class_display.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let sources = external
        .iter()
        .map(|window| (window.handle as HWND, window.capture_affinity != 0))
        .chain(
            helpers
                .iter()
                .enumerate()
                .map(|(index, window)| (window.hwnd, index == 0)),
        )
        .take(window_count)
        .collect::<Vec<_>>();
    cover.warm()?;
    let foreground = unsafe { wm::GetForegroundWindow() };

    let started = Instant::now();
    cover.show_topmost(work)?;
    paint_frame(cover.hwnd, window_count, 0.0, work)?;
    pump_messages();
    let flush = unsafe { dwm::DwmFlush() };
    if flush < 0 {
        bail!("DwmFlush failed with HRESULT {flush:#x}")
    }
    let latency = started.elapsed();
    let sentinel_colors = cover_sentinel_colors(work)?;
    if sentinel_colors.iter().any(|color| *color != COVER_COLOR) {
        let owner = sentinel_owner(work);
        bail!(
            "cover sentinels were not opaque after DwmFlush: expected={COVER_COLOR:#010x} actual={sentinel_colors:#010x?} owner={owner}"
        )
    }

    let cover_completed = Instant::now();
    let motion_started = cover_completed;
    let original_geometry = helpers
        .iter()
        .map(OwnedWindow::rect)
        .collect::<Result<Vec<_>>>()?;
    let prepared_geometry = shifted(&original_geometry, 12, 6);
    let first_geometry_batch_started = Instant::now();
    apply_batch(&helpers, &prepared_geometry)?;
    let source_geometry_before_cover = first_geometry_batch_started < cover_completed;

    let mut thumbnails = Vec::with_capacity(window_count);
    for (index, (source, protected)) in sources.iter().copied().enumerate() {
        thumbnails.push(
            (!protected && index < live_limit)
                .then(|| {
                    Thumbnail::try_register(cover.hwnd, source, tile(index, window_count, work))
                })
                .flatten(),
        );
    }
    let live_thumbnail_count = thumbnails
        .iter()
        .filter(|thumbnail| thumbnail.is_some())
        .count();
    let placeholder_count = window_count - live_thumbnail_count;

    let presentation_duration = match scenario {
        Scenario::Cancel => Duration::from_millis(90),
        Scenario::Normal | Scenario::ContentLoss => Duration::from_millis(180),
    };
    let mut frame_intervals = Vec::new();
    let mut content_loss_index = None;
    let mut content_loss_replaced_next_frame = None;
    let mut previous = Instant::now();
    while motion_started.elapsed() < presentation_duration {
        let elapsed = motion_started.elapsed().as_secs_f32();
        let progress = (elapsed / presentation_duration.as_secs_f32()).clamp(0.0, 1.0);
        if scenario == Scenario::ContentLoss && elapsed >= 0.090 && content_loss_index.is_none() {
            content_loss_index = thumbnails.iter().rposition(Option::is_some);
            if let Some(index) = content_loss_index
                && let Some(thumbnail) = &mut thumbnails[index]
            {
                thumbnail.hide()?;
            }
        }
        paint_frame(cover.hwnd, window_count, progress, work)?;
        for (index, thumbnail) in thumbnails.iter_mut().enumerate() {
            let mut destination = tile(index, window_count, work);
            destination.left += (80.0 * progress) as i32;
            destination.right += (80.0 * progress) as i32;
            if Some(index) != content_loss_index
                && let Some(thumbnail) = thumbnail
            {
                thumbnail.update(destination)?;
            }
        }
        pump_messages();
        let flush = unsafe { dwm::DwmFlush() };
        if flush < 0 {
            bail!("DwmFlush failed with HRESULT {flush:#x}")
        }
        let now = Instant::now();
        frame_intervals.push(now.duration_since(previous));
        previous = now;
        if let Some(index) = content_loss_index
            && content_loss_replaced_next_frame.is_none()
        {
            content_loss_replaced_next_frame = Some(
                screen_pixel(tile_center_screen(index, window_count, work))? == PLACEHOLDER_COLOR,
            );
            previous = Instant::now();
        }
    }

    let final_geometry = shifted(&original_geometry, 20, 10);
    apply_batch(&helpers, &final_geometry)?;
    let observed_geometry = helpers
        .iter()
        .map(OwnedWindow::rect)
        .collect::<Result<Vec<_>>>()?;
    let final_geometry_exact = observed_geometry
        .iter()
        .zip(&final_geometry)
        .all(|(observed, expected)| rect_eq(*observed, *expected));
    cover.hide();
    pump_messages();
    let flush = unsafe { dwm::DwmFlush() };
    if flush < 0 {
        bail!("cover retirement DwmFlush failed with HRESULT {flush:#x}")
    }
    let cover_retirement = cover_completed.elapsed();
    for helper in &helpers {
        helper.hide();
    }
    drop(thumbnails);
    let foreground_preserved = unsafe { wm::GetForegroundWindow() } == foreground;
    let refresh_hz = native::current_display_mode()?.refresh_hz;
    let refresh_interval = Duration::from_secs_f64(1.0 / f64::from(refresh_hz));
    let retirement_deadline =
        presentation_duration + (refresh_interval * 2).max(Duration::from_millis(50));
    let cleanup_complete = unsafe { wm::IsWindowVisible(cover.hwnd) } == 0;
    Ok(SmokeReport {
        scenario,
        window_count,
        live_limit,
        refresh_hz,
        cover_latency_ms: latency.as_secs_f64() * 1000.0,
        frame_count: frame_intervals.len(),
        frame_intervals_ms: frame_intervals
            .iter()
            .map(|interval| interval.as_secs_f64() * 1000.0)
            .collect(),
        frame_interval_p95_ms: percentile_ms(&frame_intervals, 0.95),
        consecutive_over_two_intervals: consecutive_over(&frame_intervals, refresh_interval * 2),
        sampled_duration_ms: presentation_duration.as_secs_f64() * 1000.0,
        cover_retirement_ms: cover_retirement.as_secs_f64() * 1000.0,
        cover_retirement_deadline_ms: retirement_deadline.as_secs_f64() * 1000.0,
        cover_retired_within_deadline: cover_retirement <= retirement_deadline,
        foreground_preserved,
        opaque_sentinels: true,
        unique_source_count: sources.len(),
        external_source_count: external.len().min(window_count),
        external_source_classes,
        live_thumbnail_count,
        placeholder_count,
        source_geometry_before_cover,
        placement_batch_count: 2,
        final_geometry_exact,
        content_loss_replaced_next_frame,
        cleanup_complete,
    })
}

struct WindowClass {
    atom: u16,
    name: Vec<u16>,
    instance: *mut core::ffi::c_void,
}

impl WindowClass {
    fn register() -> Result<Self> {
        let name = wide("KomorebiCoverMotionProbe");
        let instance = unsafe { GetModuleHandleW(ptr::null()) };
        if instance.is_null() {
            return Err(std::io::Error::last_os_error()).context("GetModuleHandleW");
        }
        let class = wm::WNDCLASSEXW {
            cbSize: u32::try_from(size_of::<wm::WNDCLASSEXW>())?,
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            hCursor: unsafe { wm::LoadCursorW(ptr::null_mut(), wm::IDC_ARROW) },
            lpszClassName: name.as_ptr(),
            ..Default::default()
        };
        let atom = unsafe { wm::RegisterClassExW(&class) };
        if atom == 0 {
            return Err(std::io::Error::last_os_error()).context("RegisterClassExW");
        }
        Ok(Self {
            atom,
            name,
            instance,
        })
    }

    fn create_cover(&self, work: RECT) -> Result<OwnedWindow> {
        let window = self.create(WindowSpec {
            color: COVER_COLOR,
            ex_style: wm::WS_EX_NOACTIVATE | wm::WS_EX_TOOLWINDOW | wm::WS_EX_TOPMOST,
            style: wm::WS_POPUP,
            rect: work,
        })?;
        let corner = dwm::DWMWCP_DONOTROUND;
        let result = unsafe {
            dwm::DwmSetWindowAttribute(
                window.hwnd,
                dwm::DWMWA_WINDOW_CORNER_PREFERENCE as u32,
                (&corner as *const i32).cast(),
                u32::try_from(size_of::<i32>())?,
            )
        };
        if result < 0 {
            bail!("disable cover rounding failed with HRESULT {result:#x}")
        }
        Ok(window)
    }

    fn create_helpers(&self, count: usize, work: RECT) -> Result<Vec<OwnedWindow>> {
        (0..count)
            .map(|index| {
                let x = work.left + 20 + i32::try_from(index % 10)? * 230;
                let y = work.top + 20 + i32::try_from(index / 10)? * 130;
                let window = self.create(WindowSpec {
                    color: helper_color(index),
                    ex_style: wm::WS_EX_NOACTIVATE | wm::WS_EX_TOOLWINDOW,
                    style: wm::WS_POPUP,
                    rect: RECT {
                        left: x,
                        top: y,
                        right: x + 210,
                        bottom: y + 110,
                    },
                })?;
                window.show();
                if index == 0 {
                    let succeeded = unsafe {
                        wm::SetWindowDisplayAffinity(window.hwnd, wm::WDA_EXCLUDEFROMCAPTURE)
                    };
                    if succeeded == 0 {
                        return Err(std::io::Error::last_os_error())
                            .context("SetWindowDisplayAffinity helper");
                    }
                }
                let succeeded = unsafe {
                    wm::SetWindowPos(
                        window.hwnd,
                        wm::HWND_BOTTOM,
                        x,
                        y,
                        210,
                        110,
                        wm::SWP_NOACTIVATE,
                    )
                };
                if succeeded == 0 {
                    return Err(std::io::Error::last_os_error()).context("SetWindowPos helper");
                }
                Ok(window)
            })
            .collect()
    }

    fn create(&self, spec: WindowSpec) -> Result<OwnedWindow> {
        let hwnd = unsafe {
            wm::CreateWindowExW(
                spec.ex_style,
                self.name.as_ptr(),
                self.name.as_ptr(),
                spec.style,
                spec.rect.left,
                spec.rect.top,
                spec.rect.right - spec.rect.left,
                spec.rect.bottom - spec.rect.top,
                ptr::null_mut(),
                ptr::null_mut(),
                self.instance,
                ptr::null(),
            )
        };
        if hwnd.is_null() {
            return Err(std::io::Error::last_os_error()).context("CreateWindowExW");
        }
        unsafe { wm::SetWindowLongPtrW(hwnd, wm::GWLP_USERDATA, spec.color as isize) };
        Ok(OwnedWindow { hwnd })
    }
}

struct WindowSpec {
    color: u32,
    ex_style: u32,
    style: u32,
    rect: RECT,
}

impl Drop for WindowClass {
    fn drop(&mut self) {
        unsafe { wm::UnregisterClassW(self.name.as_ptr(), self.instance) };
        let _ = self.atom;
    }
}

struct OwnedWindow {
    hwnd: HWND,
}

impl OwnedWindow {
    fn warm(&self) -> Result<()> {
        let succeeded = unsafe {
            wm::SetWindowPos(
                self.hwnd,
                wm::HWND_TOPMOST,
                -32_000,
                -32_000,
                1,
                1,
                wm::SWP_NOACTIVATE | wm::SWP_SHOWWINDOW,
            )
        };
        if succeeded == 0 {
            return Err(std::io::Error::last_os_error()).context("warm cover off-screen");
        }
        unsafe {
            gdi::InvalidateRect(self.hwnd, ptr::null(), TRUE);
            gdi::UpdateWindow(self.hwnd);
        }
        let flush = unsafe { dwm::DwmFlush() };
        if flush < 0 {
            bail!("warm cover DwmFlush failed with HRESULT {flush:#x}")
        }
        self.hide();
        let flush = unsafe { dwm::DwmFlush() };
        if flush < 0 {
            bail!("hide warm cover DwmFlush failed with HRESULT {flush:#x}")
        }
        Ok(())
    }

    fn show(&self) {
        unsafe {
            wm::ShowWindow(self.hwnd, wm::SW_SHOWNOACTIVATE);
            gdi::InvalidateRect(self.hwnd, ptr::null(), TRUE);
            gdi::UpdateWindow(self.hwnd);
        }
    }

    fn hide(&self) {
        unsafe { wm::ShowWindow(self.hwnd, wm::SW_HIDE) };
    }

    fn show_topmost(&self, rect: RECT) -> Result<()> {
        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;
        let succeeded = unsafe {
            wm::SetWindowPos(
                self.hwnd,
                wm::HWND_TOPMOST,
                rect.left,
                rect.top,
                width,
                height,
                wm::SWP_NOACTIVATE | wm::SWP_SHOWWINDOW,
            )
        };
        if succeeded == 0 {
            return Err(std::io::Error::last_os_error()).context("show topmost cover");
        }
        let region = unsafe { gdi::CreateRectRgn(0, 0, width, height) };
        if region.is_null() {
            return Err(std::io::Error::last_os_error()).context("CreateRectRgn cover");
        }
        if unsafe { gdi::SetWindowRgn(self.hwnd, region, TRUE) } == 0 {
            unsafe { gdi::DeleteObject(region) };
            return Err(std::io::Error::last_os_error()).context("SetWindowRgn cover");
        }
        unsafe {
            gdi::InvalidateRect(self.hwnd, ptr::null(), TRUE);
            gdi::UpdateWindow(self.hwnd);
        }
        Ok(())
    }

    fn rect(&self) -> Result<RECT> {
        let mut rect = RECT::default();
        if unsafe { wm::GetWindowRect(self.hwnd, &mut rect) } == 0 {
            return Err(std::io::Error::last_os_error()).context("GetWindowRect helper");
        }
        Ok(rect)
    }
}

impl Drop for OwnedWindow {
    fn drop(&mut self) {
        unsafe { wm::DestroyWindow(self.hwnd) };
    }
}

struct Thumbnail {
    handle: isize,
}

impl Thumbnail {
    fn try_register(destination: HWND, source: HWND, rect: RECT) -> Option<Self> {
        let mut handle = 0;
        let result = unsafe { dwm::DwmRegisterThumbnail(destination, source, &mut handle) };
        if result < 0 {
            return None;
        }
        let mut thumbnail = Self { handle };
        thumbnail.update(rect).ok()?;
        Some(thumbnail)
    }

    fn update(&mut self, rect: RECT) -> Result<()> {
        let properties = dwm::DWM_THUMBNAIL_PROPERTIES {
            dwFlags: dwm::DWM_TNP_RECTDESTINATION
                | dwm::DWM_TNP_VISIBLE
                | dwm::DWM_TNP_OPACITY
                | dwm::DWM_TNP_SOURCECLIENTAREAONLY,
            rcDestination: rect,
            opacity: u8::MAX,
            fVisible: TRUE,
            fSourceClientAreaOnly: TRUE,
            ..Default::default()
        };
        let result = unsafe { dwm::DwmUpdateThumbnailProperties(self.handle, &properties) };
        if result < 0 {
            bail!("DwmUpdateThumbnailProperties failed with HRESULT {result:#x}")
        }
        Ok(())
    }

    fn hide(&mut self) -> Result<()> {
        let properties = dwm::DWM_THUMBNAIL_PROPERTIES {
            dwFlags: dwm::DWM_TNP_VISIBLE,
            fVisible: 0,
            ..Default::default()
        };
        let result = unsafe { dwm::DwmUpdateThumbnailProperties(self.handle, &properties) };
        if result < 0 {
            bail!("hide DWM thumbnail failed with HRESULT {result:#x}")
        }
        Ok(())
    }
}

impl Drop for Thumbnail {
    fn drop(&mut self) {
        unsafe { dwm::DwmUnregisterThumbnail(self.handle) };
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        wm::WM_ERASEBKGND => 1,
        wm::WM_PAINT => {
            let mut paint = gdi::PAINTSTRUCT::default();
            let dc = unsafe { gdi::BeginPaint(hwnd, &mut paint) };
            let mut rect = RECT::default();
            unsafe { wm::GetClientRect(hwnd, &mut rect) };
            let color = unsafe { wm::GetWindowLongPtrW(hwnd, wm::GWLP_USERDATA) } as u32;
            let brush = unsafe { gdi::CreateSolidBrush(color) };
            if !brush.is_null() {
                unsafe {
                    gdi::FillRect(dc, &rect, brush);
                    gdi::DeleteObject(brush);
                }
            }
            unsafe { gdi::EndPaint(hwnd, &paint) };
            0
        }
        _ => unsafe { wm::DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

fn pump_messages() {
    let mut message = wm::MSG::default();
    while unsafe { wm::PeekMessageW(&mut message, ptr::null_mut(), 0, 0, wm::PM_REMOVE) } != 0 {
        unsafe {
            wm::TranslateMessage(&message);
            wm::DispatchMessageW(&message);
        }
    }
}

fn cover_sentinel_colors(work: RECT) -> Result<Vec<u32>> {
    let dc = unsafe { gdi::GetDC(ptr::null_mut()) };
    if dc.is_null() {
        return Err(std::io::Error::last_os_error()).context("GetDC desktop");
    }
    let points = [
        (work.left + 16, work.top + 16),
        (work.right - 17, work.top + 16),
        (work.left + 16, work.bottom - 17),
        (work.right - 17, work.bottom - 17),
        ((work.left + work.right) / 2, (work.top + work.bottom) / 2),
    ];
    let colors = points
        .into_iter()
        .map(|(x, y)| unsafe { gdi::GetPixel(dc, x, y) })
        .collect();
    unsafe { gdi::ReleaseDC(ptr::null_mut(), dc) };
    Ok(colors)
}

fn screen_pixel(point: POINT) -> Result<u32> {
    let dc = unsafe { gdi::GetDC(ptr::null_mut()) };
    if dc.is_null() {
        return Err(std::io::Error::last_os_error()).context("GetDC desktop");
    }
    let color = unsafe { gdi::GetPixel(dc, point.x, point.y) };
    unsafe { gdi::ReleaseDC(ptr::null_mut(), dc) };
    Ok(color)
}

fn tile_center_screen(index: usize, count: usize, work: RECT) -> POINT {
    let rect = tile(index, count, work);
    POINT {
        x: work.left + (rect.left + rect.right) / 2,
        y: work.top + (rect.top + rect.bottom) / 2,
    }
}

fn sentinel_owner(work: RECT) -> String {
    let point = POINT {
        x: work.left + 16,
        y: work.top + 16,
    };
    let hwnd = unsafe { wm::WindowFromPoint(point) };
    if hwnd.is_null() {
        return "none".to_owned();
    }
    let mut class = vec![0u16; 256];
    let class_len = unsafe { wm::GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32) };
    class.truncate(class_len.max(0) as usize);
    let title_len = unsafe { wm::GetWindowTextLengthW(hwnd) };
    let mut title = vec![0u16; title_len.max(0) as usize + 1];
    let title_copied = unsafe { wm::GetWindowTextW(hwnd, title.as_mut_ptr(), title.len() as i32) };
    title.truncate(title_copied.max(0) as usize);
    format!(
        "hwnd={:#x} class={:?} title={:?}",
        hwnd as usize,
        String::from_utf16_lossy(&class),
        String::from_utf16_lossy(&title)
    )
}

fn paint_frame(cover: HWND, count: usize, progress: f32, work: RECT) -> Result<()> {
    let dc = unsafe { gdi::GetDC(cover) };
    if dc.is_null() {
        return Err(std::io::Error::last_os_error()).context("GetDC cover");
    }
    let client = RECT {
        left: 0,
        top: 0,
        right: work.right - work.left,
        bottom: work.bottom - work.top,
    };
    let background = unsafe { gdi::CreateSolidBrush(COVER_COLOR) };
    let placeholder = unsafe { gdi::CreateSolidBrush(PLACEHOLDER_COLOR) };
    if background.is_null() || placeholder.is_null() {
        unsafe {
            if !background.is_null() {
                gdi::DeleteObject(background);
            }
            if !placeholder.is_null() {
                gdi::DeleteObject(placeholder);
            }
            gdi::ReleaseDC(cover, dc);
        }
        bail!("CreateSolidBrush failed")
    }
    unsafe { gdi::FillRect(dc, &client, background) };
    for index in 0..count {
        let mut rect = tile(index, count, work);
        rect.left += (80.0 * progress) as i32;
        rect.right += (80.0 * progress) as i32;
        unsafe { gdi::FillRect(dc, &rect, placeholder) };
    }
    unsafe {
        gdi::DeleteObject(background);
        gdi::DeleteObject(placeholder);
        gdi::ReleaseDC(cover, dc);
    }
    Ok(())
}

fn tile(index: usize, count: usize, work: RECT) -> RECT {
    let columns = if count <= 20 { 5 } else { 10 };
    let rows = count.div_ceil(columns);
    let gap = 32;
    let width = (work.right - work.left - gap * (columns as i32 + 1)) / columns as i32;
    let height = (work.bottom - work.top - gap * (rows as i32 + 1)) / rows as i32;
    let column = (index % columns) as i32;
    let row = (index / columns) as i32;
    let left = gap + column * (width + gap);
    let top = gap + row * (height + gap);
    RECT {
        left,
        top,
        right: left + width,
        bottom: top + height,
    }
}

fn helper_color(index: usize) -> u32 {
    let red = 40 + (index * 37 % 180) as u32;
    let green = 40 + (index * 67 % 180) as u32;
    let blue = 40 + (index * 97 % 180) as u32;
    red | (green << 8) | (blue << 16)
}

fn percentile_ms(samples: &[Duration], percentile: f64) -> f64 {
    let mut values = samples
        .iter()
        .map(Duration::as_secs_f64)
        .collect::<Vec<_>>();
    values.sort_by(f64::total_cmp);
    let rank = (values.len() as f64 * percentile).ceil() as usize;
    let index = rank.saturating_sub(1);
    values.get(index).copied().unwrap_or_default() * 1000.0
}

fn shifted(rects: &[RECT], x: i32, y: i32) -> Vec<RECT> {
    rects
        .iter()
        .map(|rect| RECT {
            left: rect.left + x,
            top: rect.top + y,
            right: rect.right + x,
            bottom: rect.bottom + y,
        })
        .collect()
}

fn rect_eq(left: RECT, right: RECT) -> bool {
    left.left == right.left
        && left.top == right.top
        && left.right == right.right
        && left.bottom == right.bottom
}

fn apply_batch(windows: &[OwnedWindow], rectangles: &[RECT]) -> Result<()> {
    if windows.len() != rectangles.len() {
        bail!("window and rectangle counts differ")
    }
    let count = i32::try_from(windows.len())?;
    let mut batch = unsafe { wm::BeginDeferWindowPos(count) };
    if batch.is_null() {
        return Err(std::io::Error::last_os_error()).context("BeginDeferWindowPos");
    }
    for (window, rect) in windows.iter().zip(rectangles) {
        batch = unsafe {
            wm::DeferWindowPos(
                batch,
                window.hwnd,
                ptr::null_mut(),
                rect.left,
                rect.top,
                rect.right - rect.left,
                rect.bottom - rect.top,
                wm::SWP_NOACTIVATE | wm::SWP_NOOWNERZORDER | wm::SWP_NOZORDER,
            )
        };
        if batch.is_null() {
            return Err(std::io::Error::last_os_error()).context("DeferWindowPos");
        }
    }
    if unsafe { wm::EndDeferWindowPos(batch) } == 0 {
        return Err(std::io::Error::last_os_error()).context("EndDeferWindowPos");
    }
    Ok(())
}

fn consecutive_over(samples: &[Duration], threshold: Duration) -> usize {
    samples
        .windows(2)
        .filter(|pair| pair[0] > threshold && pair[1] > threshold)
        .count()
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain([0]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn p95_uses_nearest_rank_instead_of_the_maximum() {
        let samples = (1..=20).map(Duration::from_millis).collect::<Vec<_>>();

        assert_eq!(percentile_ms(&samples, 0.95), 19.0);
    }

    #[test]
    fn every_supported_tile_stays_inside_the_cover() {
        let work = RECT {
            left: 0,
            top: 0,
            right: 5120,
            bottom: 1440,
        };

        for count in [20, 50] {
            for index in 0..count {
                let rect = tile(index, count, work);
                assert!(rect.left >= 0);
                assert!(rect.top >= 0);
                assert!(rect.right <= work.right);
                assert!(rect.bottom <= work.bottom);
                assert!(rect.left < rect.right);
                assert!(rect.top < rect.bottom);
            }
        }
    }
}
