use std::ffi::OsStr;
use std::ffi::OsString;

use windows_sys::Win32::System::RemoteDesktop::ProcessIdToSessionId;
use windows_sys::Win32::System::Threading::GetCurrentProcessId;

use crate::TransportError;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct WindowsSessionId(u32);

impl WindowsSessionId {
    /// Resolves the Windows session that owns the current process.
    ///
    /// # Errors
    ///
    /// Returns a Windows error when the process-to-session lookup fails.
    pub fn current() -> Result<Self, TransportError> {
        let mut value = 0;
        // SAFETY: `value` is a valid writable u32 and the process ID is supplied by Windows.
        let succeeded = unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &raw mut value) };
        if succeeded == 0 {
            Err(TransportError::windows("ProcessIdToSessionId"))
        } else {
            Ok(Self(value))
        }
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }

    pub(crate) const fn from_raw(value: u32) -> Self {
        Self(value)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandPipeEndpoint(OsString);

impl CommandPipeEndpoint {
    /// Builds the command endpoint for the current Windows session.
    ///
    /// # Errors
    ///
    /// Returns a Windows error when the current session cannot be resolved.
    pub fn current() -> Result<Self, TransportError> {
        Ok(Self::for_session(WindowsSessionId::current()?))
    }

    #[must_use]
    pub fn for_session(session: WindowsSessionId) -> Self {
        Self(
            format!(
                r"\\.\pipe\LOCAL\komorebi-command-v1-session-{}",
                session.get()
            )
            .into(),
        )
    }

    #[must_use]
    pub fn as_os_str(&self) -> &OsStr {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_is_local_and_session_scoped() {
        let endpoint = CommandPipeEndpoint::for_session(WindowsSessionId(42));
        assert_eq!(
            endpoint.as_os_str(),
            OsStr::new(r"\\.\pipe\LOCAL\komorebi-command-v1-session-42")
        );
    }
}
