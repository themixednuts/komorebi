use std::num::NonZeroU32;
use std::os::windows::io::AsRawHandle;

use komorebi_protocol::PrincipalId;
use sha2::Digest;
use sha2::Sha256;
use tokio::net::windows::named_pipe::NamedPipeServer;
use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::Security::RevertToSelf;
use windows_sys::Win32::System::Pipes::GetNamedPipeClientProcessId;
use windows_sys::Win32::System::Pipes::GetNamedPipeClientSessionId;
use windows_sys::Win32::System::Pipes::ImpersonateNamedPipeClient;

use crate::LogonSid;
use crate::TransportError;
use crate::WindowsSessionId;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeerIdentity {
    process_id: NonZeroU32,
    session_id: WindowsSessionId,
    logon_sid: LogonSid,
    principal_id: PrincipalId,
}

impl PeerIdentity {
    pub(crate) fn authenticate(
        pipe: &NamedPipeServer,
        expected_logon: &LogonSid,
        expected_session: WindowsSessionId,
    ) -> Result<Self, TransportError> {
        let handle = pipe.as_raw_handle().cast();
        let process_id = client_process_id(handle)?;
        let session_id = client_session_id(handle)?;
        let impersonation = ImpersonationGuard::begin(handle)?;
        let logon_sid = LogonSid::current_thread();
        impersonation.revert();
        let logon_sid = logon_sid?;
        if &logon_sid != expected_logon {
            return Err(TransportError::WrongLogon {
                expected: expected_logon.clone(),
                actual: logon_sid,
            });
        }
        if session_id != expected_session {
            return Err(TransportError::WrongSession {
                expected: expected_session.get(),
                actual: session_id.get(),
            });
        }
        let principal_id = PrincipalId::new(Sha256::digest(logon_sid.as_bytes()).into())?;
        Ok(Self {
            process_id,
            session_id,
            logon_sid,
            principal_id,
        })
    }

    #[must_use]
    pub const fn process_id(&self) -> NonZeroU32 {
        self.process_id
    }

    #[must_use]
    pub const fn session_id(&self) -> WindowsSessionId {
        self.session_id
    }

    #[must_use]
    pub const fn logon_sid(&self) -> &LogonSid {
        &self.logon_sid
    }

    /// Returns the stable protocol principal derived from the authenticated
    /// Windows logon SID, never from client-supplied bootstrap data.
    #[must_use]
    pub const fn principal_id(&self) -> PrincipalId {
        self.principal_id
    }
}

fn client_process_id(handle: HANDLE) -> Result<NonZeroU32, TransportError> {
    let mut process_id = 0;
    // SAFETY: `handle` is a connected named-pipe server and output is writable.
    if unsafe { GetNamedPipeClientProcessId(handle, &raw mut process_id) } == 0 {
        return Err(TransportError::windows("GetNamedPipeClientProcessId"));
    }
    NonZeroU32::new(process_id).ok_or(TransportError::ZeroClientProcessId)
}

fn client_session_id(handle: HANDLE) -> Result<WindowsSessionId, TransportError> {
    let mut session_id = 0;
    // SAFETY: `handle` is a connected named-pipe server and output is writable.
    if unsafe { GetNamedPipeClientSessionId(handle, &raw mut session_id) } == 0 {
        Err(TransportError::windows("GetNamedPipeClientSessionId"))
    } else {
        Ok(WindowsSessionId::from_raw(session_id))
    }
}

struct ImpersonationGuard {
    active: bool,
}

impl ImpersonationGuard {
    fn begin(handle: HANDLE) -> Result<Self, TransportError> {
        // SAFETY: `handle` is a connected named-pipe server handle.
        if unsafe { ImpersonateNamedPipeClient(handle) } == 0 {
            Err(TransportError::windows("ImpersonateNamedPipeClient"))
        } else {
            Ok(Self { active: true })
        }
    }

    fn revert(mut self) {
        self.revert_or_abort();
    }

    fn revert_or_abort(&mut self) {
        if self.active {
            // SAFETY: this thread is impersonating exactly one named-pipe client.
            if unsafe { RevertToSelf() } == 0 {
                // Windows explicitly requires process shutdown if reverting fails;
                // continuing would execute manager work with client authority.
                std::process::abort();
            }
            self.active = false;
        }
    }
}

impl Drop for ImpersonationGuard {
    fn drop(&mut self) {
        self.revert_or_abort();
    }
}
