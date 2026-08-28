use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Scenario {
    Normal,
    Cancel,
    ContentLoss,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct DisplayMode {
    pub width_px: u32,
    pub height_px: u32,
    pub refresh_hz: u32,
    pub bits_per_pixel: u32,
}

#[derive(Debug, Serialize)]
pub struct WindowCandidate {
    pub handle: isize,
    pub process_id: u32,
    pub class_utf16: Vec<u16>,
    pub title_utf16: Vec<u16>,
    pub class_display: String,
    pub title_display: String,
    pub minimized: bool,
    pub capture_affinity: u32,
}

#[derive(Debug, Serialize)]
pub struct Inventory {
    pub current_mode: DisplayMode,
    pub available_modes: Vec<DisplayMode>,
    pub windows: Vec<WindowCandidate>,
}

#[derive(Debug, Serialize)]
pub struct SmokeReport {
    pub scenario: Scenario,
    pub window_count: usize,
    pub live_limit: usize,
    pub refresh_hz: u32,
    pub cover_latency_ms: f64,
    pub frame_count: usize,
    pub frame_intervals_ms: Vec<f64>,
    pub frame_interval_p95_ms: f64,
    pub consecutive_over_two_intervals: usize,
    pub sampled_duration_ms: f64,
    pub cover_retirement_ms: f64,
    pub cover_retirement_deadline_ms: f64,
    pub cover_retired_within_deadline: bool,
    pub foreground_preserved: bool,
    pub opaque_sentinels: bool,
    pub unique_source_count: usize,
    pub external_source_count: usize,
    pub external_source_classes: Vec<String>,
    pub live_thumbnail_count: usize,
    pub placeholder_count: usize,
    pub source_geometry_before_cover: bool,
    pub placement_batch_count: usize,
    pub final_geometry_exact: bool,
    pub content_loss_replaced_next_frame: Option<bool>,
    pub cleanup_complete: bool,
}

#[derive(Debug, Serialize)]
pub struct MatrixReport {
    pub current_mode: DisplayMode,
    pub available_modes: Vec<DisplayMode>,
    pub requested_refresh_hz: Vec<u32>,
    pub unavailable_refresh_hz: Vec<u32>,
    pub repetitions: usize,
    pub trials: Vec<SmokeReport>,
}
