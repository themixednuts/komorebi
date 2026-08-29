use std::{
    os::windows::ffi::OsStrExt as _,
    path::{Path, PathBuf},
};

use anyhow::{Context as _, Result};
use serde::Serialize;
use tokio::io::AsyncWriteExt as _;
use windows_sys::Win32::Storage::FileSystem::{
    MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
};

use crate::matrix::{CompilerArm, PINNED_NIGHTLY, Scope};

#[derive(Debug, Serialize)]
struct CompilerReport<'a> {
    complete: bool,
    source_revision: &'a str,
    stable_version: &'a str,
    nightly_version: &'a str,
    pinned_nightly: &'static str,
    trials: usize,
    measurements: &'a [CommandMeasurement],
    diagnostics: &'a [DiagnosticMeasurement],
}

pub(crate) struct ReportContext {
    pub(crate) source_revision: String,
    pub(crate) stable_version: String,
    pub(crate) nightly_version: String,
    pub(crate) trials: usize,
}

#[derive(Debug, Serialize)]
pub(crate) struct CommandMeasurement {
    pub(crate) trial: usize,
    pub(crate) arm: CompilerArm,
    pub(crate) scope: Scope,
    pub(crate) operation: &'static str,
    pub(crate) elapsed_ms: u64,
    pub(crate) success: bool,
    pub(crate) exit_code: Option<i32>,
    pub(crate) warning_lines: usize,
    pub(crate) error_lines: usize,
    pub(crate) release_binary_bytes: Option<u64>,
    pub(crate) stderr_tail: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct DiagnosticMeasurement {
    pub(crate) arm: CompilerArm,
    pub(crate) success: bool,
    pub(crate) exit_code: Option<i32>,
    pub(crate) json_diagnostic_lines: usize,
    pub(crate) output_path: String,
}

pub(crate) async fn write_report(
    path: &Path,
    complete: bool,
    context: &ReportContext,
    measurements: &[CommandMeasurement],
    diagnostics: &[DiagnosticMeasurement],
) -> Result<()> {
    let report = CompilerReport {
        complete,
        source_revision: &context.source_revision,
        stable_version: &context.stable_version,
        nightly_version: &context.nightly_version,
        pinned_nightly: PINNED_NIGHTLY,
        trials: context.trials,
        measurements,
        diagnostics,
    };
    let bytes = serde_json::to_vec_pretty(&report)?;
    atomic_write(path, &bytes).await
}

async fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let temporary = temporary_path(path);
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&temporary)
        .await
        .with_context(|| format!("open report checkpoint {}", temporary.display()))?;
    file.write_all(bytes)
        .await
        .context("write report checkpoint")?;
    file.sync_all().await.context("flush report checkpoint")?;
    drop(file);

    let source = wide_nul(&temporary);
    let destination = wide_nul(path);
    // SAFETY: both buffers are NUL-terminated and remain alive for the call. The source and
    // destination are files on the same volume, so replacement is atomic to observers.
    let moved = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if moved == 0 {
        let error = std::io::Error::last_os_error();
        let _ = tokio::fs::remove_file(&temporary).await;
        return Err(error).context("atomically replace compiler measurement report");
    }
    Ok(())
}

fn temporary_path(path: &Path) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(format!(".{}.tmp", std::process::id()));
    value.into()
}

fn wide_nul(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}
