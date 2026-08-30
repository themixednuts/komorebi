use std::mem::size_of;
use std::ptr;
use std::slice;

use windows_sys::Win32::Foundation::LocalFree;
use windows_sys::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW;
use windows_sys::Win32::Security::Authorization::SDDL_REVISION_1;
use windows_sys::Win32::Security::GetSecurityDescriptorLength;
use windows_sys::Win32::Security::PSECURITY_DESCRIPTOR;
use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;

use crate::LogonSid;
use crate::TransportError;

pub(crate) struct PipeSecurityDescriptor(Box<[u8]>);

impl PipeSecurityDescriptor {
    pub(crate) fn for_logon(logon_sid: &LogonSid) -> Result<Self, TransportError> {
        let sddl = format!("D:P(A;;GA;;;{})", logon_sid.to_sddl()?);
        let mut wide: Vec<u16> = sddl.encode_utf16().collect();
        wide.push(0);

        let mut descriptor = ptr::null_mut();
        // SAFETY: `wide` is NUL terminated and the output pointer is writable.
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                wide.as_ptr(),
                SDDL_REVISION_1,
                &raw mut descriptor,
                ptr::null_mut(),
            )
        } == 0
        {
            return Err(TransportError::windows(
                "ConvertStringSecurityDescriptorToSecurityDescriptorW",
            ));
        }
        let allocation = LocalSecurityDescriptor(descriptor);
        // SAFETY: Windows returned a valid self-relative security descriptor.
        let length = unsafe { GetSecurityDescriptorLength(allocation.0) };
        let length = usize::try_from(length).map_err(|_| TransportError::MalformedToken)?;
        if length == 0 {
            return Err(TransportError::MalformedToken);
        }
        // SAFETY: GetSecurityDescriptorLength returned the initialized allocation size.
        let bytes = unsafe { slice::from_raw_parts(allocation.0.cast::<u8>(), length) };
        Ok(Self(bytes.into()))
    }

    pub(crate) fn attributes(&self) -> Result<SECURITY_ATTRIBUTES, TransportError> {
        Ok(SECURITY_ATTRIBUTES {
            nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>())
                .map_err(|_| TransportError::MalformedToken)?,
            lpSecurityDescriptor: self.0.as_ptr().cast_mut().cast(),
            bInheritHandle: 0,
        })
    }
}

struct LocalSecurityDescriptor(PSECURITY_DESCRIPTOR);

impl Drop for LocalSecurityDescriptor {
    fn drop(&mut self) {
        // SAFETY: ConvertStringSecurityDescriptorToSecurityDescriptorW allocates
        // this descriptor with LocalAlloc and ownership has not been transferred.
        let _ = unsafe { LocalFree(self.0.cast()) };
    }
}
