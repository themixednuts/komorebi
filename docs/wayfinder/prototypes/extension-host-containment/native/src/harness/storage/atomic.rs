use std::ffi::OsStr;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::ptr::null;

use anyhow::{Context, Result, ensure};
use uuid::Uuid;
use windows_sys::Win32::Foundation::ERROR_FILE_NOT_FOUND;
use windows_sys::Win32::Storage::FileSystem::{
    MOVEFILE_WRITE_THROUGH, MoveFileExW, REPLACEFILE_WRITE_THROUGH, ReplaceFileW,
};

use crate::windows::wide;

pub(super) struct AtomicFiles {
    directory: PathBuf,
    active: PathBuf,
    backup: PathBuf,
}

impl AtomicFiles {
    pub(super) fn create(directory: PathBuf) -> Result<Self> {
        fs::create_dir_all(&directory)
            .with_context(|| format!("create storage directory {}", directory.display()))?;
        Ok(Self {
            active: directory.join("store.json"),
            backup: directory.join("store.backup.json"),
            directory,
        })
    }

    pub(super) fn read_active(&self, maximum_bytes: usize) -> Result<Option<Vec<u8>>> {
        match OpenOptions::new().read(true).open(&self.active) {
            Ok(file) => {
                let read_limit = maximum_bytes
                    .checked_add(1)
                    .context("storage snapshot read limit overflow")?;
                let mut bytes = Vec::new();
                file.take(u64::try_from(read_limit)?)
                    .read_to_end(&mut bytes)
                    .context("read active extension store")?;
                ensure!(
                    bytes.len() <= maximum_bytes,
                    "extension storage snapshot exceeds its encoded size limit"
                );
                Ok(Some(bytes))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error).context("read active extension store"),
        }
    }

    pub(super) fn stage(&self, bytes: &[u8]) -> Result<StagedFile> {
        let path = self
            .directory
            .join(format!("{}.stage", Uuid::new_v4().simple()));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .context("create unique storage stage")?;
        file.write_all(bytes).context("write storage stage")?;
        file.sync_all().context("sync storage stage")?;
        drop(file);
        Ok(StagedFile {
            path,
            remove_on_drop: true,
        })
    }

    pub(super) fn rollback(&self) -> Result<()> {
        let active = wide(&self.active)?;
        let backup = wide(&self.backup)?;
        // SAFETY: both paths are NUL-terminated; ReplaceFileW performs one atomic replacement.
        if unsafe {
            ReplaceFileW(
                active.as_ptr(),
                backup.as_ptr(),
                null(),
                REPLACEFILE_WRITE_THROUGH,
                null(),
                null(),
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error()).context("roll back extension store");
        }
        Ok(())
    }

    pub(super) fn remove_orphan_stages(&self) -> Result<usize> {
        let mut removed = 0_usize;
        for entry in
            fs::read_dir(&self.directory).context("enumerate storage recovery directory")?
        {
            let entry = entry.context("read storage recovery entry")?;
            let path = entry.path();
            if path.extension() != Some(OsStr::new("stage")) {
                continue;
            }
            fs::remove_file(&path).context("remove orphaned storage stage")?;
            removed = removed
                .checked_add(1)
                .context("orphaned-stage count overflow")?;
        }
        Ok(removed)
    }

    fn promote(&self, staged: &Path) -> Result<()> {
        let active = wide(&self.active)?;
        let replacement = wide(staged)?;
        let backup = wide(&self.backup)?;
        // SAFETY: paths are NUL-terminated and staged is a fully synced replacement file.
        if unsafe {
            ReplaceFileW(
                active.as_ptr(),
                replacement.as_ptr(),
                backup.as_ptr(),
                REPLACEFILE_WRITE_THROUGH,
                null(),
                null(),
            )
        } != 0
        {
            return Ok(());
        }
        let replace_error = std::io::Error::last_os_error();
        if replace_error.raw_os_error() != Some(ERROR_FILE_NOT_FOUND.cast_signed()) {
            return Err(replace_error).context("atomically replace extension store");
        }
        // SAFETY: paths are NUL-terminated; no replace flag means a racing destination fails.
        if unsafe {
            MoveFileExW(
                replacement.as_ptr(),
                active.as_ptr(),
                MOVEFILE_WRITE_THROUGH,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error()).context("install first extension store");
        }
        Ok(())
    }
}

pub(super) struct StagedFile {
    path: PathBuf,
    remove_on_drop: bool,
}

impl StagedFile {
    pub(super) fn promote(mut self, files: &AtomicFiles) -> Result<()> {
        files.promote(&self.path)?;
        self.remove_on_drop = false;
        Ok(())
    }

    pub(super) fn abandon_for_recovery_test(mut self) {
        self.remove_on_drop = false;
    }
}

impl Drop for StagedFile {
    fn drop(&mut self) {
        if self.remove_on_drop {
            // A failed stage is best-effort cleanup; the next open also removes this exact suffix.
            if let Err(error) = fs::remove_file(&self.path)
                && error.kind() != std::io::ErrorKind::NotFound
            {
                eprintln!("failed to remove extension storage stage: {error}");
            }
        }
    }
}
