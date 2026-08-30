use std::io;

use komorebi_protocol::BootstrapCodecError;
use komorebi_protocol::CatalogTransferError;
use komorebi_protocol::CommandCodecError;
use komorebi_protocol::ConnectionId;
use komorebi_protocol::FrameError;
use komorebi_protocol::FrameKind;
use komorebi_protocol::IdentifierError;
use komorebi_protocol::InvocationIdentityError;
use komorebi_protocol::ProtocolFaultCode;
use komorebi_protocol::SequenceError;
use komorebi_protocol::StreamId;
use komorebi_protocol::TraceId;
use komorebi_protocol::UnsupportedVersion;
use thiserror::Error;

use crate::LogonSid;

#[derive(Debug, Error)]
pub enum TransportError {
    #[error("{operation} failed")]
    Windows {
        operation: &'static str,
        #[source]
        source: io::Error,
    },
    #[error("Windows returned malformed token data")]
    MalformedToken,
    #[error("the access token has no logon SID")]
    MissingLogonSid,
    #[error("named-pipe client process ID is zero")]
    ZeroClientProcessId,
    #[error("named-pipe peer belongs to logon {actual:?}, expected {expected:?}")]
    WrongLogon {
        expected: LogonSid,
        actual: LogonSid,
    },
    #[error("named-pipe peer belongs to Windows session {actual}, expected {expected}")]
    WrongSession { expected: u32, actual: u32 },
    #[error("connection closed while reading a protocol value")]
    UnexpectedEof,
    #[error("connection made no progress while writing a protocol value")]
    WriteZero,
    #[error("a frame is already queued for this connection")]
    WriteInProgress,
    #[error("bootstrap must use the control stream, received {0:?}")]
    BootstrapMustUseControlStream(StreamId),
    #[error("expected first bootstrap frame {expected:?}, received {actual:?}")]
    UnexpectedBootstrapFrame {
        expected: FrameKind,
        actual: FrameKind,
    },
    #[error("the server does not support this command protocol: {0:?}")]
    UnsupportedVersion(UnsupportedVersion),
    #[error("the server rejected the command protocol with {code:?} ({trace_id:?})")]
    ProtocolFault {
        code: ProtocolFaultCode,
        trace_id: TraceId,
    },
    #[error("the server selected a command protocol outside the client offer")]
    NegotiationMismatch,
    #[error("another client request is already in progress")]
    ClientRequestInProgress,
    #[error("client-owned stream identities are exhausted")]
    ClientStreamsExhausted,
    #[error("expected reply stream {expected:?}, received {actual:?}")]
    UnexpectedReplyStream {
        expected: StreamId,
        actual: StreamId,
    },
    #[error("expected reply frame {expected:?}, received {actual:?}")]
    UnexpectedReplyFrame {
        expected: FrameKind,
        actual: FrameKind,
    },
    #[error("catalog transfer exceeded the negotiated assembly deadline")]
    CatalogAssemblyDeadline,
    #[error("requests must use a client-initiated stream, received {0:?}")]
    RequestMustUseClientStream(StreamId),
    #[error("unsupported request frame kind {0:?}")]
    UnsupportedRequestFrame(FrameKind),
    #[error("reply target belongs to connection {actual:?}, expected {expected:?}")]
    WrongReplyConnection {
        expected: ConnectionId,
        actual: ConnectionId,
    },
    #[error(transparent)]
    CommandPayload(#[from] CommandCodecError),
    #[error(transparent)]
    CatalogTransfer(#[from] CatalogTransferError),
    #[error(transparent)]
    Bootstrap(#[from] BootstrapCodecError),
    #[error(transparent)]
    Identifier(#[from] IdentifierError),
    #[error(transparent)]
    InvocationIdentity(#[from] InvocationIdentityError),
    #[error(transparent)]
    Frame(#[from] FrameError),
    #[error(transparent)]
    Sequence(#[from] SequenceError),
    #[error(transparent)]
    Io(#[from] io::Error),
}

impl TransportError {
    pub(crate) fn windows(operation: &'static str) -> Self {
        Self::Windows {
            operation,
            source: io::Error::last_os_error(),
        }
    }

    pub(crate) fn io(operation: &'static str, source: io::Error) -> Self {
        Self::Windows { operation, source }
    }
}
