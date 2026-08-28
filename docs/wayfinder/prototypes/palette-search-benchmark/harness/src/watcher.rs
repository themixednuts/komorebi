use std::ffi::c_void;
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::sync::mpsc::SyncSender;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_LIST_DIRECTORY, FILE_NOTIFY_CHANGE_FILE_NAME,
    FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    OPEN_EXISTING, ReadDirectoryChangesW,
};
use windows::core::PCWSTR;

use crate::native::{NativeError, OwnedHandle};

const WATCH_BUFFER_BYTES: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WatchInvalidation {
    Changed { bytes: u32 },
    Overflow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotStatus {
    StaleNeedsReplacement,
}

#[must_use]
pub const fn invalidate_snapshot(event: WatchInvalidation) -> SnapshotStatus {
    match event {
        WatchInvalidation::Changed { .. } | WatchInvalidation::Overflow => {
            SnapshotStatus::StaleNeedsReplacement
        }
    }
}

pub fn wait_for_one_invalidation(
    root: &Path,
    armed: &SyncSender<()>,
) -> Result<WatchInvalidation, WatchError> {
    let mut wide = root.as_os_str().encode_wide().collect::<Vec<_>>();
    if wide.contains(&0) {
        return Err(WatchError::InteriorNul);
    }
    wide.push(0);
    // SAFETY: path is NUL-terminated and this handle requests directory notifications only.
    let directory = unsafe {
        CreateFileW(
            PCWSTR(wide.as_ptr()),
            FILE_LIST_DIRECTORY.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            None,
        )
    }?;
    let directory = OwnedHandle::new(directory)?;
    armed.send(()).map_err(|_| WatchError::ObserverGone)?;

    // A u32 backing allocation meets FILE_NOTIFY_INFORMATION's DWORD alignment contract.
    let words = WATCH_BUFFER_BYTES
        .checked_div(size_of::<u32>())
        .ok_or(WatchError::BufferSize)?;
    let mut buffer = vec![0u32; words];
    let mut bytes = 0u32;
    // SAFETY: directory is a live directory handle, the aligned buffer remains writable for the
    // blocking call, bytes is writable, and no completion callback or OVERLAPPED is supplied.
    unsafe {
        ReadDirectoryChangesW(
            directory.raw(),
            buffer.as_mut_ptr().cast::<c_void>(),
            u32::try_from(WATCH_BUFFER_BYTES).map_err(|_| WatchError::BufferSize)?,
            true,
            FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_LAST_WRITE,
            Some(&raw mut bytes),
            None,
            None,
        )
    }?;
    if bytes == 0 {
        Ok(WatchInvalidation::Overflow)
    } else {
        Ok(WatchInvalidation::Changed { bytes })
    }
}

#[derive(Debug, Error)]
pub enum WatchError {
    #[error("native watcher operation failed")]
    Native(#[from] NativeError),
    #[error("Windows watcher operation failed")]
    Windows(#[from] windows::core::Error),
    #[error("watch root contains an interior NUL")]
    InteriorNul,
    #[error("watch buffer size is invalid")]
    BufferSize,
    #[error("watch observer disappeared before the handle was armed")]
    ObserverGone,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overflow_invalidates_instead_of_replaying_partial_history() {
        assert_eq!(
            invalidate_snapshot(WatchInvalidation::Overflow),
            SnapshotStatus::StaleNeedsReplacement
        );
    }
}
