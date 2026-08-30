use std::ffi::c_void;
use std::mem;

use komorebi_protocol::Frame;
use komorebi_protocol::FrameHeader;
use komorebi_protocol::FrameKind;
use komorebi_protocol::HEADER_BYTES;
use komorebi_protocol::InboundSequence;
use komorebi_protocol::OutboundSequence;
use komorebi_protocol::ProtocolPreface;
use komorebi_protocol::SequenceError;
use komorebi_protocol::StreamId;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::windows::named_pipe::NamedPipeServer;
use tokio::net::windows::named_pipe::PipeMode;
use tokio::net::windows::named_pipe::ServerOptions;

use crate::CommandPipeEndpoint;
use crate::LogonSid;
use crate::PeerIdentity;
use crate::TransportError;
use crate::WindowsSessionId;
use crate::security::PipeSecurityDescriptor;

const PREFACE_BYTES: usize = 8;
const PIPE_BUFFER_BYTES: u32 = 64 * 1024;

pub struct CommandPipeListener {
    endpoint: CommandPipeEndpoint,
    session_id: WindowsSessionId,
    logon_sid: LogonSid,
    security: PipeSecurityDescriptor,
    pending: NamedPipeServer,
}

impl CommandPipeListener {
    /// Claims the current session's command endpoint for the current logon.
    ///
    /// # Errors
    ///
    /// Returns an error when identity or DACL construction fails, another
    /// server owns the first instance, or Tokio cannot register the pipe.
    pub fn bind_current() -> Result<Self, TransportError> {
        let session_id = WindowsSessionId::current()?;
        let endpoint = CommandPipeEndpoint::for_session(session_id);
        let logon_sid = LogonSid::current()?;
        let security = PipeSecurityDescriptor::for_logon(&logon_sid)?;
        let pending = create_instance(&endpoint, &security, InstanceOwnership::Claim)?;
        Ok(Self {
            endpoint,
            session_id,
            logon_sid,
            security,
            pending,
        })
    }

    /// Waits on the Windows I/O completion port for one connection.
    ///
    /// Tokio guarantees that named-pipe `connect` is cancellation safe. The
    /// next unconnected instance is created before the authenticated connection
    /// is returned so a listener remains continuously available.
    ///
    /// # Errors
    ///
    /// Returns an error when connection completion, replacement-instance
    /// creation, or Windows-derived peer authentication fails.
    pub async fn accept(&mut self) -> Result<AuthenticatedPipe, TransportError> {
        self.pending
            .connect()
            .await
            .map_err(|error| TransportError::io("ConnectNamedPipe", error))?;
        let next = create_instance(
            &self.endpoint,
            &self.security,
            InstanceOwnership::Additional,
        )?;
        let connected = mem::replace(&mut self.pending, next);
        let peer = PeerIdentity::authenticate(&connected, &self.logon_sid, self.session_id)?;
        Ok(AuthenticatedPipe {
            pipe: connected,
            peer,
        })
    }

    #[must_use]
    pub const fn endpoint(&self) -> &CommandPipeEndpoint {
        &self.endpoint
    }
}

fn create_instance(
    endpoint: &CommandPipeEndpoint,
    security: &PipeSecurityDescriptor,
    ownership: InstanceOwnership,
) -> Result<NamedPipeServer, TransportError> {
    let mut attributes = security.attributes()?;
    let mut options = ServerOptions::new();
    options
        .first_pipe_instance(matches!(ownership, InstanceOwnership::Claim))
        .pipe_mode(PipeMode::Byte)
        .reject_remote_clients(true)
        .in_buffer_size(PIPE_BUFFER_BYTES)
        .out_buffer_size(PIPE_BUFFER_BYTES);
    // SAFETY: `attributes` and its self-relative security descriptor remain
    // alive for the synchronous CreateNamedPipeW call made by Tokio. Tokio owns
    // the resulting overlapped handle and registers it with the runtime IOCP.
    unsafe {
        options.create_with_security_attributes_raw(
            endpoint.as_os_str(),
            (&raw mut attributes).cast::<c_void>(),
        )
    }
    .map_err(|error| TransportError::io("CreateNamedPipeW", error))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InstanceOwnership {
    Claim,
    Additional,
}

pub struct AuthenticatedPipe {
    pipe: NamedPipeServer,
    peer: PeerIdentity,
}

impl AuthenticatedPipe {
    /// Validates the protocol preface before exposing frame operations.
    ///
    /// Cancellation drops this not-yet-protocol connection, so partially read
    /// preface bytes can never be reused as a frame boundary.
    ///
    /// # Errors
    ///
    /// Returns an error on EOF, pipe I/O failure, or an invalid preface.
    pub async fn begin_protocol(mut self) -> Result<ProtocolConnection, TransportError> {
        let mut preface = [0; PREFACE_BYTES];
        read_complete(&mut self.pipe, &mut preface).await?;
        ProtocolPreface::decode(&preface)?;
        Ok(ProtocolConnection {
            pipe: self.pipe,
            peer: self.peer,
            read: FrameReadState::default(),
            inbound_sequence: InboundSequence::default(),
            write: None,
            outbound_sequence: OutboundSequence::default(),
            outbound_preface: OutboundPreface::Pending,
        })
    }

    #[must_use]
    pub const fn peer(&self) -> &PeerIdentity {
        &self.peer
    }
}

pub struct ProtocolConnection {
    pipe: NamedPipeServer,
    peer: PeerIdentity,
    read: FrameReadState,
    inbound_sequence: InboundSequence,
    write: Option<PendingWrite>,
    outbound_sequence: OutboundSequence,
    outbound_preface: OutboundPreface,
}

impl ProtocolConnection {
    #[must_use]
    pub const fn peer(&self) -> &PeerIdentity {
        &self.peer
    }

    /// Receives one bounded frame while retaining partial progress in `self`.
    ///
    /// Each underlying Tokio `read` is cancellation safe. Dropping this future
    /// between reads preserves the completed prefix and the next call resumes
    /// at exactly the next byte.
    ///
    /// # Errors
    ///
    /// Returns an error on EOF, pipe I/O failure, or invalid frame data.
    pub async fn receive_frame(&mut self) -> Result<Frame, TransportError> {
        loop {
            let (pipe, state) = (&mut self.pipe, &mut self.read);
            match state {
                FrameReadState::Header { bytes, filled } => {
                    read_progress(pipe, bytes, filled).await?;
                    if *filled == bytes.len() {
                        let header = FrameHeader::decode(bytes)?;
                        let payload = vec![0; header.payload_len()];
                        if payload.is_empty() {
                            *state = FrameReadState::default();
                            let frame = Frame::from_received_parts(header, payload)?;
                            self.inbound_sequence.accept(frame.header().sequence())?;
                            return Ok(frame);
                        }
                        *state = FrameReadState::Payload {
                            header,
                            bytes: payload,
                            filled: 0,
                        };
                    }
                }
                FrameReadState::Payload {
                    header,
                    bytes,
                    filled,
                } => {
                    read_progress(pipe, bytes, filled).await?;
                    if *filled == bytes.len() {
                        let header = *header;
                        let payload = mem::take(bytes);
                        *state = FrameReadState::default();
                        let frame = Frame::from_received_parts(header, payload)?;
                        self.inbound_sequence.accept(frame.header().sequence())?;
                        return Ok(frame);
                    }
                }
            }
        }
    }

    /// Creates and queues exactly one correctly sequenced frame.
    ///
    /// # Errors
    ///
    /// Returns [`TransportError::WriteInProgress`] when a previous frame has
    /// not finished flushing, or a framing/sequence error at its boundary.
    pub fn queue_frame(
        &mut self,
        kind: FrameKind,
        stream_id: StreamId,
        payload: impl Into<Box<[u8]>>,
    ) -> Result<komorebi_protocol::DirectionSequence, TransportError> {
        if self.write.is_some() {
            return Err(TransportError::WriteInProgress);
        }
        let sequence = self
            .outbound_sequence
            .next()
            .ok_or(SequenceError::Exhausted)?;
        let frame = Frame::new(kind, stream_id, sequence, payload)?;
        let preface = self.outbound_preface.take();
        let capacity =
            preface.as_ref().map_or(0, |bytes| bytes.len()) + HEADER_BYTES + frame.payload().len();
        let mut bytes = Vec::with_capacity(capacity);
        if let Some(preface) = preface {
            bytes.extend_from_slice(&preface);
        }
        bytes.extend_from_slice(&frame.header().encode());
        bytes.extend_from_slice(frame.payload());
        self.write = Some(PendingWrite { bytes, written: 0 });
        let issued = self.outbound_sequence.issue()?;
        debug_assert_eq!(issued, sequence);
        Ok(sequence)
    }

    /// Flushes the queued frame and retains its offset if cancelled.
    ///
    /// # Errors
    ///
    /// Returns an error when the pipe write fails or makes no progress.
    pub async fn flush_queued_frame(&mut self) -> Result<(), TransportError> {
        while let Some(pending) = &mut self.write {
            let written = self.pipe.write(&pending.bytes[pending.written..]).await?;
            if written == 0 {
                return Err(TransportError::WriteZero);
            }
            pending.written += written;
            if pending.written == pending.bytes.len() {
                self.write = None;
            }
        }
        Ok(())
    }

    #[must_use]
    pub const fn has_queued_frame(&self) -> bool {
        self.write.is_some()
    }
}

enum FrameReadState {
    Header {
        bytes: [u8; HEADER_BYTES],
        filled: usize,
    },
    Payload {
        header: FrameHeader,
        bytes: Vec<u8>,
        filled: usize,
    },
}

impl Default for FrameReadState {
    fn default() -> Self {
        Self::Header {
            bytes: [0; HEADER_BYTES],
            filled: 0,
        }
    }
}

struct PendingWrite {
    bytes: Vec<u8>,
    written: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OutboundPreface {
    Pending,
    Sent,
}

impl OutboundPreface {
    fn take(&mut self) -> Option<[u8; PREFACE_BYTES]> {
        match self {
            Self::Pending => {
                *self = Self::Sent;
                Some(ProtocolPreface.encode())
            }
            Self::Sent => None,
        }
    }
}

async fn read_complete(pipe: &mut NamedPipeServer, bytes: &mut [u8]) -> Result<(), TransportError> {
    let mut filled = 0;
    read_progress(pipe, bytes, &mut filled).await
}

async fn read_progress(
    pipe: &mut NamedPipeServer,
    bytes: &mut [u8],
    filled: &mut usize,
) -> Result<(), TransportError> {
    while *filled < bytes.len() {
        let read = pipe.read(&mut bytes[*filled..]).await?;
        if read == 0 {
            return Err(TransportError::UnexpectedEof);
        }
        *filled += read;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use komorebi_protocol::DirectionSequence;
    use komorebi_protocol::FrameKind;
    use komorebi_protocol::StreamId;
    use tokio::io::AsyncReadExt;
    use tokio::io::AsyncWriteExt;
    use tokio::net::windows::named_pipe::ClientOptions;
    use windows_sys::Win32::System::Threading::GetCurrentProcessId;

    use super::*;

    #[tokio::test(flavor = "current_thread")]
    #[serial_test::serial]
    async fn live_pipe_authenticates_and_round_trips_a_frame()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut listener = CommandPipeListener::bind_current()?;
        assert!(CommandPipeListener::bind_current().is_err());
        let mut client = ClientOptions::new().open(listener.endpoint().as_os_str())?;
        let authenticated = listener.accept().await?;
        // SAFETY: GetCurrentProcessId has no preconditions.
        let current_process_id = unsafe { GetCurrentProcessId() };
        assert_eq!(
            authenticated.peer().process_id(),
            NonZeroU32::new(current_process_id).ok_or(TransportError::ZeroClientProcessId)?
        );
        assert_eq!(
            authenticated.peer().session_id(),
            WindowsSessionId::current()?
        );
        assert_eq!(authenticated.peer().logon_sid(), &LogonSid::current()?);

        let request = Frame::new(
            FrameKind::new(11),
            StreamId::client_initiated(NonZeroU32::MIN)?,
            DirectionSequence::try_from(1)?,
            &b"request"[..],
        )?;
        client.write_all(&ProtocolPreface.encode()).await?;
        let request_header = request.header().encode();
        client.write_all(&request_header[..10]).await?;

        let mut protocol = authenticated.begin_protocol().await?;
        let canceled = tokio::select! {
            biased;
            result = protocol.receive_frame() => Some(result),
            () = std::future::ready(()) => None,
        };
        assert!(canceled.is_none());
        client.write_all(&request_header[10..]).await?;
        client.write_all(request.payload()).await?;
        assert_eq!(protocol.receive_frame().await?, request);

        client.write_all(&request_header).await?;
        client.write_all(request.payload()).await?;
        assert!(matches!(
            protocol.receive_frame().await,
            Err(TransportError::Sequence(
                komorebi_protocol::SequenceError::Replay { .. }
            ))
        ));

        let response_stream =
            StreamId::server_initiated(NonZeroU32::new(2).ok_or(TransportError::MalformedToken)?)?;
        let response_sequence =
            protocol.queue_frame(FrameKind::new(12), response_stream, &b"response"[..])?;
        let response = Frame::new(
            FrameKind::new(12),
            response_stream,
            response_sequence,
            &b"response"[..],
        )?;
        protocol.flush_queued_frame().await?;

        let mut received = vec![0; PREFACE_BYTES + HEADER_BYTES + response.payload().len()];
        client.read_exact(&mut received).await?;
        ProtocolPreface::decode(&received[..PREFACE_BYTES])?;
        let header = FrameHeader::decode(&received[PREFACE_BYTES..PREFACE_BYTES + HEADER_BYTES])?;
        let payload = &received[PREFACE_BYTES + HEADER_BYTES..];
        assert_eq!(Frame::from_received_parts(header, payload)?, response);
        Ok(())
    }
}
