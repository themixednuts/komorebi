use komorebi_protocol::AuthoritySummary;
use komorebi_protocol::BootstrapCodec;
use komorebi_protocol::ConnectionId;
use komorebi_protocol::HELLO_FRAME_KIND;
use komorebi_protocol::ManagerEpoch;
use komorebi_protocol::NegotiatedProtocol;
use komorebi_protocol::ProtocolNegotiator;
use komorebi_protocol::ServerSupport;
use komorebi_protocol::StreamId;
use komorebi_protocol::UNSUPPORTED_VERSION_FRAME_KIND;
use komorebi_protocol::UnsupportedVersion;
use komorebi_protocol::WELCOME_FRAME_KIND;
use komorebi_protocol::Welcome;
use uuid::Uuid;

use crate::AuthenticatedPipe;
use crate::CommandPipeEndpoint;
use crate::CommandPipeListener;
use crate::PeerIdentity;
use crate::ProtocolConnection;
use crate::TransportError;

#[derive(Clone)]
struct ServerContext {
    support: ServerSupport,
    manager_epoch: ManagerEpoch,
    authority: AuthoritySummary,
}

/// Owns endpoint publication and immutable bootstrap policy for one manager.
pub struct CommandProtocolServer {
    listener: CommandPipeListener,
    context: ServerContext,
}

impl CommandProtocolServer {
    /// Claims the current session endpoint with one process-lifetime epoch.
    ///
    /// # Errors
    ///
    /// Returns [`TransportError`] when the endpoint, current logon identity, or
    /// manager epoch cannot be established.
    pub fn bind_current(
        support: ServerSupport,
        authority: AuthoritySummary,
    ) -> Result<Self, TransportError> {
        let manager_epoch = ManagerEpoch::new(*Uuid::new_v4().as_bytes())?;
        Ok(Self {
            listener: CommandPipeListener::bind_current()?,
            context: ServerContext {
                support,
                manager_epoch,
                authority,
            },
        })
    }

    /// Accepts and authenticates one pipe without waiting for its handshake.
    ///
    /// The caller can immediately resume accepting and negotiate the returned
    /// session in an owned task, so a slow peer cannot block endpoint admission.
    ///
    /// # Errors
    ///
    /// Returns [`TransportError`] when pipe completion or Windows-derived peer
    /// authentication fails.
    pub async fn accept(&mut self) -> Result<PendingProtocolSession, TransportError> {
        let pipe = self.listener.accept().await?;
        let connection_id = ConnectionId::new(*Uuid::new_v4().as_bytes())?;
        Ok(PendingProtocolSession {
            pipe,
            context: self.context.clone(),
            connection_id,
        })
    }

    #[must_use]
    pub const fn endpoint(&self) -> &CommandPipeEndpoint {
        self.listener.endpoint()
    }

    #[must_use]
    pub const fn manager_epoch(&self) -> ManagerEpoch {
        self.context.manager_epoch
    }
}

/// A Windows-authenticated connection whose bootstrap bytes are not trusted yet.
pub struct PendingProtocolSession {
    pipe: AuthenticatedPipe,
    context: ServerContext,
    connection_id: ConnectionId,
}

impl PendingProtocolSession {
    /// Validates the preface and first `Hello`, then sends exactly one bootstrap
    /// outcome on the reserved control stream.
    ///
    /// Cancellation is safe: dropping this future drops only this connection;
    /// the listener has already installed its next named-pipe instance.
    ///
    /// # Errors
    ///
    /// Returns [`TransportError`] for malformed framing, sequencing, bootstrap
    /// CBOR, or a non-`Hello` first frame.
    pub async fn negotiate(self) -> Result<SessionAcceptance, TransportError> {
        let peer = self.pipe.peer().clone();
        let mut connection = self.pipe.begin_protocol().await?;
        let hello_frame = connection.receive_frame().await?;
        if hello_frame.header().stream_id() != StreamId::Control {
            return Err(TransportError::BootstrapMustUseControlStream(
                hello_frame.header().stream_id(),
            ));
        }
        if hello_frame.header().kind() != HELLO_FRAME_KIND {
            return Err(TransportError::UnexpectedBootstrapFrame {
                expected: HELLO_FRAME_KIND,
                actual: hello_frame.header().kind(),
            });
        }
        let hello = BootstrapCodec::decode_hello(hello_frame.payload())?;

        let Ok(negotiated) = ProtocolNegotiator::select(&self.context.support, &hello) else {
            let unsupported = UnsupportedVersion::new(
                self.context.support.protocol_versions().clone(),
                self.context.support.catalog_schemas().clone(),
            );
            let payload = BootstrapCodec::encode_unsupported_version(&unsupported)?;
            connection.queue_frame(UNSUPPORTED_VERSION_FRAME_KIND, StreamId::Control, payload)?;
            connection.flush_queued_frame().await?;
            return Ok(SessionAcceptance::Unsupported { peer });
        };

        let welcome = Welcome::new(
            negotiated.clone(),
            self.context.manager_epoch,
            self.connection_id,
            self.context.authority,
        );
        let payload = BootstrapCodec::encode_welcome(&welcome)?;
        connection.queue_frame(WELCOME_FRAME_KIND, StreamId::Control, payload)?;
        connection.flush_queued_frame().await?;
        Ok(SessionAcceptance::Established(Box::new(
            EstablishedSession {
                connection,
                negotiated,
                manager_epoch: self.context.manager_epoch,
                connection_id: self.connection_id,
            },
        )))
    }
}

pub enum SessionAcceptance {
    Established(Box<EstablishedSession>),
    Unsupported { peer: PeerIdentity },
}

pub struct EstablishedSession {
    connection: ProtocolConnection,
    negotiated: NegotiatedProtocol,
    manager_epoch: ManagerEpoch,
    connection_id: ConnectionId,
}

impl EstablishedSession {
    #[must_use]
    pub const fn peer(&self) -> &PeerIdentity {
        self.connection.peer()
    }

    #[must_use]
    pub const fn negotiated(&self) -> &NegotiatedProtocol {
        &self.negotiated
    }

    #[must_use]
    pub const fn manager_epoch(&self) -> ManagerEpoch {
        self.manager_epoch
    }

    #[must_use]
    pub const fn connection_id(&self) -> ConnectionId {
        self.connection_id
    }

    #[must_use]
    pub fn into_connection(self) -> ProtocolConnection {
        self.connection
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU16;

    use komorebi_protocol::CatalogSchemaVersion;
    use komorebi_protocol::FeatureSet;
    use komorebi_protocol::Frame;
    use komorebi_protocol::FrameHeader;
    use komorebi_protocol::HEADER_BYTES;
    use komorebi_protocol::ProtocolMajor;
    use komorebi_protocol::ProtocolMinor;
    use komorebi_protocol::ProtocolPreface;
    use komorebi_protocol::ProtocolVersion;
    use komorebi_protocol::SessionLimits;
    use komorebi_protocol::VersionRange;
    use komorebi_protocol::VersionRanges;
    use tokio::io::AsyncReadExt;
    use tokio::io::AsyncWriteExt;
    use tokio::net::windows::named_pipe::ClientOptions;

    use super::*;

    fn support(protocol_minor: u16) -> Result<ServerSupport, Box<dyn std::error::Error>> {
        let protocol = ProtocolVersion::new(
            ProtocolMajor::new(NonZeroU16::MIN),
            ProtocolMinor::new(protocol_minor),
        );
        let schema = CatalogSchemaVersion::new(NonZeroU16::MIN);
        Ok(ServerSupport::new(
            VersionRanges::new(vec![VersionRange::new(protocol, protocol)?])?,
            VersionRanges::new(vec![VersionRange::new(schema, schema)?])?,
            FeatureSet::default(),
            SessionLimits::V1,
        ))
    }

    fn hello(protocol_minor: u16) -> Result<komorebi_protocol::Hello, Box<dyn std::error::Error>> {
        let protocol = ProtocolVersion::new(
            ProtocolMajor::new(NonZeroU16::MIN),
            ProtocolMinor::new(protocol_minor),
        );
        let schema = CatalogSchemaVersion::new(NonZeroU16::MIN);
        Ok(komorebi_protocol::Hello::new(
            VersionRanges::new(vec![VersionRange::new(protocol, protocol)?])?,
            VersionRanges::new(vec![VersionRange::new(schema, schema)?])?,
            FeatureSet::default(),
            None,
        ))
    }

    async fn send_hello(
        client: &mut tokio::net::windows::named_pipe::NamedPipeClient,
        value: &komorebi_protocol::Hello,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let frame = Frame::new(
            HELLO_FRAME_KIND,
            StreamId::Control,
            komorebi_protocol::DirectionSequence::try_from(1)?,
            BootstrapCodec::encode_hello(value)?,
        )?;
        client.write_all(&ProtocolPreface.encode()).await?;
        client.write_all(&frame.header().encode()).await?;
        client.write_all(frame.payload()).await?;
        Ok(())
    }

    async fn receive_bootstrap(
        client: &mut tokio::net::windows::named_pipe::NamedPipeClient,
    ) -> Result<Frame, Box<dyn std::error::Error>> {
        let mut preface = [0; 8];
        client.read_exact(&mut preface).await?;
        ProtocolPreface::decode(&preface)?;
        let mut header = [0; HEADER_BYTES];
        client.read_exact(&mut header).await?;
        let header = FrameHeader::decode(&header)?;
        let mut payload = vec![0; header.payload_len()];
        client.read_exact(&mut payload).await?;
        Ok(Frame::from_received_parts(header, payload)?)
    }

    #[tokio::test(flavor = "current_thread")]
    #[serial_test::serial]
    async fn handshake_establishes_or_rejects_without_blocking_the_listener()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut server =
            CommandProtocolServer::bind_current(support(0)?, AuthoritySummary::default())?;

        let mut accepted_client = ClientOptions::new().open(server.endpoint().as_os_str())?;
        let accepted_pending = server.accept().await?;
        send_hello(&mut accepted_client, &hello(0)?).await?;

        let mut rejected_client = ClientOptions::new().open(server.endpoint().as_os_str())?;
        let rejected_pending = server.accept().await?;
        send_hello(&mut rejected_client, &hello(1)?).await?;

        let SessionAcceptance::Established(established) = accepted_pending.negotiate().await?
        else {
            return Err("matching client was rejected".into());
        };
        assert_eq!(established.manager_epoch(), server.manager_epoch());
        let welcome = receive_bootstrap(&mut accepted_client).await?;
        assert_eq!(welcome.header().kind(), WELCOME_FRAME_KIND);
        assert_eq!(
            BootstrapCodec::decode_welcome(welcome.payload())?.connection_id(),
            established.connection_id()
        );

        assert!(matches!(
            rejected_pending.negotiate().await?,
            SessionAcceptance::Unsupported { .. }
        ));
        let unsupported = receive_bootstrap(&mut rejected_client).await?;
        assert_eq!(unsupported.header().kind(), UNSUPPORTED_VERSION_FRAME_KIND);
        BootstrapCodec::decode_unsupported_version(unsupported.payload())?;
        Ok(())
    }
}
