use std::fs::{File, OpenOptions};
use std::io;
use std::time::Duration;

use windows_sys::Win32::System::Pipes::WaitNamedPipeW;

use crate::windows::wide;

pub(crate) fn open(path: &str, timeout: Duration) -> io::Result<File> {
    let path_wide = wide(path)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error.to_string()))?;
    let timeout_ms = u32::try_from(timeout.as_millis())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "pipe timeout exceeds u32"))?;
    // SAFETY: path_wide is NUL-terminated and timeout_ms is a bounded wait duration.
    if unsafe { WaitNamedPipeW(path_wide.as_ptr(), timeout_ms) } == 0 {
        return Err(io::Error::last_os_error());
    }
    OpenOptions::new().read(true).write(true).open(path)
}
