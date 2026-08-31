use std::mem::size_of;
use std::os::windows::ffi::OsStrExt as _;
use std::path::Path;
use std::path::PathBuf;

use windows::Win32::UI::Shell::SEE_MASK_FLAG_NO_UI;
use windows::Win32::UI::Shell::SEE_MASK_NOASYNC;
use windows::Win32::UI::Shell::SHELLEXECUTEINFOW;
use windows::Win32::UI::Shell::ShellExecuteExW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::PCWSTR;

use crate::FileLaunchFailure;
use crate::FileLauncher;

/// Native Windows file activation through the registered shell association.
#[derive(Clone, Copy, Debug, Default)]
pub struct WindowsFileLauncher;

impl FileLauncher for WindowsFileLauncher {
    async fn launch(&self, path: PathBuf) -> Result<(), FileLaunchFailure> {
        tokio::task::spawn_blocking(move || launch_file(&path))
            .await
            .map_err(|error| {
                FileLaunchFailure::new(format!("file-launch worker failed: {error}"))
            })?
    }
}

fn launch_file(path: &Path) -> Result<(), FileLaunchFailure> {
    let mut path = path.as_os_str().encode_wide().collect::<Vec<_>>();
    if path.contains(&0) {
        return Err(FileLaunchFailure::new(
            "file path contains an interior UTF-16 NUL",
        ));
    }
    path.push(0);
    let cb_size = u32::try_from(size_of::<SHELLEXECUTEINFOW>())
        .map_err(|_| FileLaunchFailure::new("ShellExecuteExW structure size overflowed"))?;
    let mut execution = SHELLEXECUTEINFOW {
        cbSize: cb_size,
        fMask: SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
        lpFile: PCWSTR(path.as_ptr()),
        nShow: SW_SHOWNORMAL.0,
        ..SHELLEXECUTEINFOW::default()
    };

    // SAFETY: `execution` has the documented size, every omitted pointer is
    // null, and `lpFile` points to a live terminal-NUL UTF-16 buffer for the
    // duration of this synchronous `SEE_MASK_NOASYNC` call.
    unsafe { ShellExecuteExW(&raw mut execution) }
        .map_err(|error| FileLaunchFailure::native(error.code().0, error.message().clone()))
}
