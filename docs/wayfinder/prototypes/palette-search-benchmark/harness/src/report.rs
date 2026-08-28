use std::ffi::OsStr;
use std::fs::{self, OpenOptions};
use std::io::{BufWriter, Write};
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use serde::Serialize;
use thiserror::Error;
use windows::Win32::Storage::FileSystem::{
    MOVEFILE_WRITE_THROUGH, MoveFileExW, REPLACEFILE_WRITE_THROUGH, ReplaceFileW,
};
use windows::core::PCWSTR;

pub fn publish_json<T: Serialize>(target: &Path, value: &T) -> Result<(), ReportError> {
    let parent = target.parent().ok_or(ReportError::MissingParent)?;
    fs::create_dir_all(parent)?;
    let file_name = target.file_name().ok_or(ReportError::MissingFileName)?;
    let staging = staging_path(parent, file_name);
    let publish = (|| {
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staging)?;
        let mut writer = BufWriter::new(file);
        serde_json::to_writer_pretty(&mut writer, value)?;
        writer.write_all(b"\n")?;
        writer.flush()?;
        writer.get_ref().sync_all()?;
        replace_same_directory(&staging, target)
    })();
    if publish.is_err() {
        let _ = fs::remove_file(&staging);
    }
    publish
}

fn staging_path(parent: &Path, file_name: &OsStr) -> PathBuf {
    let mut name = file_name.to_os_string();
    name.push(format!(".{}.staging", uuid::Uuid::new_v4()));
    parent.join(name)
}

fn replace_same_directory(staging: &Path, target: &Path) -> Result<(), ReportError> {
    let staging = nul_terminated(staging.as_os_str())?;
    let target_wide = nul_terminated(target.as_os_str())?;
    if target.exists() {
        // SAFETY: both paths are NUL-terminated, same-directory paths; no backup is requested.
        unsafe {
            ReplaceFileW(
                PCWSTR(target_wide.as_ptr()),
                PCWSTR(staging.as_ptr()),
                None,
                REPLACEFILE_WRITE_THROUGH,
                None,
                None,
            )
        }?;
    } else {
        // SAFETY: both paths are NUL-terminated and the target does not currently exist.
        unsafe {
            MoveFileExW(
                PCWSTR(staging.as_ptr()),
                PCWSTR(target_wide.as_ptr()),
                MOVEFILE_WRITE_THROUGH,
            )
        }?;
    }
    Ok(())
}

fn nul_terminated(value: &OsStr) -> Result<Vec<u16>, ReportError> {
    let mut wide = value.encode_wide().collect::<Vec<_>>();
    if wide.contains(&0) {
        return Err(ReportError::InteriorNul);
    }
    wide.push(0);
    Ok(wide)
}

#[derive(Debug, Error)]
pub enum ReportError {
    #[error("report path has no parent directory")]
    MissingParent,
    #[error("report path has no file name")]
    MissingFileName,
    #[error("report path contains an interior NUL")]
    InteriorNul,
    #[error("report filesystem operation failed")]
    Io(#[from] std::io::Error),
    #[error("report serialization failed")]
    Json(#[from] serde_json::Error),
    #[error("atomic Windows report publication failed")]
    Windows(#[from] windows::core::Error),
}
