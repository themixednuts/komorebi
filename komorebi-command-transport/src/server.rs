use komorebi_protocol::ActionInvocation;
use komorebi_protocol::ActionInvocationCodec;
use komorebi_protocol::AuthoritySummary;
use komorebi_protocol::BootstrapCodec;
use komorebi_protocol::CANCEL_INVOCATION_FRAME_KIND;
use komorebi_protocol::CANCEL_INVOCATION_REPLY_FRAME_KIND;
use komorebi_protocol::CATALOG_REPLY_FRAME_KIND;
use komorebi_protocol::CancelInvocationReply;
use komorebi_protocol::CancelInvocationRequest;
use komorebi_protocol::CatalogChunks;
use komorebi_protocol::CatalogCodec;
use komorebi_protocol::CatalogQuery;
use komorebi_protocol::CatalogReply;
use komorebi_protocol::ConnectionId;
use komorebi_protocol::GET_CATALOG_FRAME_KIND;
use komorebi_protocol::HELLO_FRAME_KIND;
use komorebi_protocol::INVOCATION_LEASE_REPLY_FRAME_KIND;
use komorebi_protocol::INVOCATION_STATUS_FRAME_KIND;
use komorebi_protocol::INVOCATION_STATUS_REPLY_FRAME_KIND;
use komorebi_protocol::INVOKE_ACTION_FRAME_KIND;
use komorebi_protocol::InvocationControlCodec;
use komorebi_protocol::InvocationLeaseCodec;
use komorebi_protocol::InvocationLeaseReply;
use komorebi_protocol::InvocationLeaseRequest;
use komorebi_protocol::InvocationStatusReply;
use komorebi_protocol::InvocationStatusRequest;
use komorebi_protocol::LEASE_INVOCATION_IDS_FRAME_KIND;
use komorebi_protocol::ManagerEpoch;
use komorebi_protocol::NegotiatedProtocol;
use komorebi_protocol::PrincipalId;
use komorebi_protocol::ProtocolNegotiator;
use komorebi_protocol::ServerSupport;
use komorebi_protocol::SessionLimits;
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
    /// Returns [`TransportError`] when the endpoint or current logon identity
    /// cannot be established.
    pub fn bind_current(
        manager_epoch: ManagerEpoch,
        support: ServerSupport,
        authority: AuthoritySummary,
    ) -> Result<Self, TransportError> {
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
            self.context.authority.clone(),
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
                pending_reply: None,
                authority: SessionAuthority {
                    principal: peer.principal_id(),
                    capabilities: self.context.authority,
                },
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
    pending_reply: Option<PendingReply>,
    authority: SessionAuthority,
}

#[derive(Debug, Eq, PartialEq)]
pub enum SessionRequest {
    Invoke(ActionInvocation),
    LeaseInvocationIds(InvocationLeaseRequest),
    InvocationStatus(InvocationStatusRequest),
    CancelInvocation(CancelInvocationRequest),
    GetCatalog(CatalogQuery),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionReply {
    InvocationLease(InvocationLeaseReply),
    InvocationStatus(InvocationStatusReply),
    CancelInvocation(CancelInvocationReply),
    Catalog(CatalogReply),
}

struct PendingReply {
    stream_id: StreamId,
    frames: ReplyFrames,
}

enum ReplyFrames {
    Single(Option<EncodedReplyFrame>),
    Catalog(CatalogChunks),
}

struct EncodedReplyFrame {
    kind: komorebi_protocol::FrameKind,
    payload: Box<[u8]>,
}

impl PendingReply {
    fn new(
        stream_id: StreamId,
        reply: SessionReply,
        limits: SessionLimits,
    ) -> Result<Self, TransportError> {
        let frames = match reply {
            SessionReply::InvocationLease(reply) => ReplyFrames::single(
                INVOCATION_LEASE_REPLY_FRAME_KIND,
                InvocationLeaseCodec::encode_reply(reply)?,
            ),
            SessionReply::InvocationStatus(reply) => ReplyFrames::single(
                INVOCATION_STATUS_REPLY_FRAME_KIND,
                InvocationControlCodec::encode_status_reply(reply)?,
            ),
            SessionReply::CancelInvocation(reply) => ReplyFrames::single(
                CANCEL_INVOCATION_REPLY_FRAME_KIND,
                InvocationControlCodec::encode_cancel_reply(reply)?,
            ),
            SessionReply::Catalog(reply) => {
                ReplyFrames::Catalog(CatalogChunks::new(&reply, limits)?)
            }
        };
        Ok(Self { stream_id, frames })
    }

    fn next_frame(&mut self) -> Result<Option<EncodedReplyFrame>, TransportError> {
        match &mut self.frames {
            ReplyFrames::Single(frame) => Ok(frame.take()),
            ReplyFrames::Catalog(chunks) => {
                Ok(chunks.next_chunk()?.map(|chunk| EncodedReplyFrame {
                    kind: CATALOG_REPLY_FRAME_KIND,
                    payload: chunk.encode(),
                }))
            }
        }
    }
}

impl ReplyFrames {
    fn single(kind: komorebi_protocol::FrameKind, payload: Vec<u8>) -> Self {
        Self::Single(Some(EncodedReplyFrame {
            kind,
            payload: payload.into_boxed_slice(),
        }))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReplyTarget {
    connection_id: ConnectionId,
    stream_id: StreamId,
}

impl ReplyTarget {
    #[must_use]
    pub const fn stream_id(self) -> StreamId {
        self.stream_id
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct AuthenticatedRequest {
    authority: SessionAuthority,
    reply_target: ReplyTarget,
    request: SessionRequest,
}

impl AuthenticatedRequest {
    #[must_use]
    pub const fn authority(&self) -> &SessionAuthority {
        &self.authority
    }

    #[must_use]
    pub const fn stream_id(&self) -> StreamId {
        self.reply_target.stream_id()
    }

    #[must_use]
    pub const fn reply_target(&self) -> ReplyTarget {
        self.reply_target
    }

    #[must_use]
    pub const fn request(&self) -> &SessionRequest {
        &self.request
    }

    #[must_use]
    pub fn into_request(self) -> SessionRequest {
        self.request
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionAuthority {
    principal: PrincipalId,
    capabilities: AuthoritySummary,
}

impl SessionAuthority {
    #[must_use]
    pub const fn principal(&self) -> PrincipalId {
        self.principal
    }

    #[must_use]
    pub const fn capabilities(&self) -> &AuthoritySummary {
        &self.capabilities
    }
}

impl EstablishedSession {
    /// Receives and validates one authenticated client request.
    ///
    /// Pipe I/O and untrusted decoding end here. The returned value owns all
    /// data needed to cross a bounded channel to the manager state owner.
    ///
    /// # Errors
    ///
    /// Returns [`TransportError`] for framing, sequence, stream ownership,
    /// unsupported kinds, or malformed request payloads.
    pub async fn receive_request(&mut self) -> Result<AuthenticatedRequest, TransportError> {
        let frame = self.connection.receive_frame().await?;
        let stream_id = frame.header().stream_id();
        if !matches!(stream_id, StreamId::ClientInitiated(_)) {
            return Err(TransportError::RequestMustUseClientStream(stream_id));
        }
        let request = match frame.header().kind() {
            INVOKE_ACTION_FRAME_KIND => {
                SessionRequest::Invoke(ActionInvocationCodec::decode(frame.payload())?)
            }
            LEASE_INVOCATION_IDS_FRAME_KIND => SessionRequest::LeaseInvocationIds(
                InvocationLeaseCodec::decode_request(frame.payload())?,
            ),
            INVOCATION_STATUS_FRAME_KIND => SessionRequest::InvocationStatus(
                InvocationControlCodec::decode_status_request(frame.payload())?,
            ),
            CANCEL_INVOCATION_FRAME_KIND => SessionRequest::CancelInvocation(
                InvocationControlCodec::decode_cancel_request(frame.payload())?,
            ),
            GET_CATALOG_FRAME_KIND => {
                SessionRequest::GetCatalog(CatalogCodec::decode_query(frame.payload())?)
            }
            kind => return Err(TransportError::UnsupportedRequestFrame(kind)),
        };
        Ok(AuthenticatedRequest {
            authority: self.authority.clone(),
            reply_target: ReplyTarget {
                connection_id: self.connection_id,
                stream_id,
            },
            request,
        })
    }

    /// Queues and flushes one typed reply on its originating request stream.
    ///
    /// Cancellation is safe: a partially written reply remains connection-owned
    /// and the next call resumes at the retained byte offset.
    ///
    /// # Errors
    ///
    /// Returns [`TransportError`] when encoding, framing, or pipe I/O fails.
    pub async fn send_reply(
        &mut self,
        target: ReplyTarget,
        reply: SessionReply,
    ) -> Result<(), TransportError> {
        if target.connection_id != self.connection_id {
            return Err(TransportError::WrongReplyConnection {
                expected: self.connection_id,
                actual: target.connection_id,
            });
        }
        self.flush_pending_reply().await?;
        self.pending_reply = Some(PendingReply::new(
            target.stream_id(),
            reply,
            self.negotiated.limits(),
        )?);
        self.flush_pending_reply().await
    }

    async fn flush_pending_reply(&mut self) -> Result<(), TransportError> {
        self.connection.flush_queued_frame().await?;
        loop {
            let next = match &mut self.pending_reply {
                Some(pending) => pending
                    .next_frame()?
                    .map(|frame| (pending.stream_id, frame)),
                None => return Ok(()),
            };
            let Some((stream_id, frame)) = next else {
                self.pending_reply = None;
                return Ok(());
            };
            self.connection
                .queue_frame(frame.kind, stream_id, frame.payload)?;
            self.connection.flush_queued_frame().await?;
        }
    }

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
    pub const fn authority(&self) -> &SessionAuthority {
        &self.authority
    }

    #[must_use]
    pub fn into_connection(self) -> ProtocolConnection {
        self.connection
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU16;
    use std::num::NonZeroU32;
    use std::num::NonZeroU64;

    use komorebi_protocol::ActionArguments;
    use komorebi_protocol::ActionContractFingerprint;
    use komorebi_protocol::ActionId;
    use komorebi_protocol::ActionInvocation;
    use komorebi_protocol::ActionInvocationCodec;
    use komorebi_protocol::ActionKey;
    use komorebi_protocol::ActionSchemaVersion;
    use komorebi_protocol::CancelInvocationRequest;
    use komorebi_protocol::CatalogCodec;
    use komorebi_protocol::CatalogQuery;
    use komorebi_protocol::CatalogReassembler;
    use komorebi_protocol::CatalogReply;
    use komorebi_protocol::CatalogSchemaVersion;
    use komorebi_protocol::CatalogStamp;
    use komorebi_protocol::FeatureSet;
    use komorebi_protocol::Frame;
    use komorebi_protocol::FrameHeader;
    use komorebi_protocol::HEADER_BYTES;
    use komorebi_protocol::InvocationControlCodec;
    use komorebi_protocol::InvocationId;
    use komorebi_protocol::InvocationLeaseCodec;
    use komorebi_protocol::InvocationLeaseRejection;
    use komorebi_protocol::InvocationLeaseReply;
    use komorebi_protocol::InvocationLeaseRequest;
    use komorebi_protocol::InvocationNamespaceId;
    use komorebi_protocol::InvocationSequence;
    use komorebi_protocol::InvocationStatusReply;
    use komorebi_protocol::InvocationStatusRequest;
    use komorebi_protocol::InvocationUnavailable;
    use komorebi_protocol::OfferRef;
    use komorebi_protocol::ProtocolMajor;
    use komorebi_protocol::ProtocolMinor;
    use komorebi_protocol::ProtocolPreface;
    use komorebi_protocol::ProtocolVersion;
    use komorebi_protocol::Revision;
    use komorebi_protocol::SessionLimits;
    use komorebi_protocol::StateStamp;
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

    async fn receive_frame(
        client: &mut tokio::net::windows::named_pipe::NamedPipeClient,
    ) -> Result<Frame, Box<dyn std::error::Error>> {
        let mut header = [0; HEADER_BYTES];
        client.read_exact(&mut header).await?;
        let header = FrameHeader::decode(&header)?;
        let mut payload = vec![0; header.payload_len()];
        client.read_exact(&mut payload).await?;
        Ok(Frame::from_received_parts(header, payload)?)
    }

    fn invocation(epoch: ManagerEpoch) -> Result<ActionInvocation, Box<dyn std::error::Error>> {
        let revision = Revision::try_from(1)?;
        Ok(ActionInvocation::new(
            InvocationId::new(
                InvocationNamespaceId::new([2; 16])?,
                InvocationSequence::new(NonZeroU64::MIN),
            ),
            OfferRef::new(
                ActionKey::new(
                    ActionId::parse("focus-window")?,
                    ActionSchemaVersion::new(NonZeroU16::MIN),
                ),
                ActionContractFingerprint::new([3; 32]),
                CatalogStamp::new(epoch, revision, revision, revision),
            ),
            StateStamp::new(epoch, revision),
            ActionArguments::default(),
            None,
        ))
    }

    async fn send_invocation(
        client: &mut tokio::net::windows::named_pipe::NamedPipeClient,
        invocation: &ActionInvocation,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let frame = Frame::new(
            INVOKE_ACTION_FRAME_KIND,
            StreamId::client_initiated(NonZeroU32::MIN)?,
            komorebi_protocol::DirectionSequence::try_from(2)?,
            ActionInvocationCodec::encode(invocation)?,
        )?;
        client.write_all(&frame.header().encode()).await?;
        client.write_all(frame.payload()).await?;
        Ok(())
    }

    async fn send_lease_request(
        client: &mut tokio::net::windows::named_pipe::NamedPipeClient,
        request: InvocationLeaseRequest,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let frame = Frame::new(
            LEASE_INVOCATION_IDS_FRAME_KIND,
            StreamId::client_initiated(NonZeroU32::MIN.saturating_add(2))?,
            komorebi_protocol::DirectionSequence::try_from(3)?,
            InvocationLeaseCodec::encode_request(request)?,
        )?;
        client.write_all(&frame.header().encode()).await?;
        client.write_all(frame.payload()).await?;
        Ok(())
    }

    async fn send_status_request(
        client: &mut tokio::net::windows::named_pipe::NamedPipeClient,
        request: InvocationStatusRequest,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let frame = Frame::new(
            INVOCATION_STATUS_FRAME_KIND,
            StreamId::client_initiated(NonZeroU32::MIN.saturating_add(4))?,
            komorebi_protocol::DirectionSequence::try_from(4)?,
            InvocationControlCodec::encode_status_request(request)?,
        )?;
        client.write_all(&frame.header().encode()).await?;
        client.write_all(frame.payload()).await?;
        Ok(())
    }

    async fn send_cancel_request(
        client: &mut tokio::net::windows::named_pipe::NamedPipeClient,
        request: CancelInvocationRequest,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let frame = Frame::new(
            CANCEL_INVOCATION_FRAME_KIND,
            StreamId::client_initiated(NonZeroU32::MIN.saturating_add(6))?,
            komorebi_protocol::DirectionSequence::try_from(5)?,
            InvocationControlCodec::encode_cancel_request(request)?,
        )?;
        client.write_all(&frame.header().encode()).await?;
        client.write_all(frame.payload()).await?;
        Ok(())
    }

    async fn send_catalog_query(
        client: &mut tokio::net::windows::named_pipe::NamedPipeClient,
        query: CatalogQuery,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let frame = Frame::new(
            GET_CATALOG_FRAME_KIND,
            StreamId::client_initiated(NonZeroU32::MIN.saturating_add(8))?,
            komorebi_protocol::DirectionSequence::try_from(6)?,
            CatalogCodec::encode_query(query)?,
        )?;
        client.write_all(&frame.header().encode()).await?;
        client.write_all(frame.payload()).await?;
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    #[serial_test::serial]
    async fn handshake_establishes_or_rejects_without_blocking_the_listener()
    -> Result<(), Box<dyn std::error::Error>> {
        let manager_epoch = ManagerEpoch::new(*Uuid::new_v4().as_bytes())?;
        let mut server = CommandProtocolServer::bind_current(
            manager_epoch,
            support(0)?,
            AuthoritySummary::default(),
        )?;
        assert_eq!(server.manager_epoch(), manager_epoch);

        let mut accepted_client = ClientOptions::new().open(server.endpoint().as_os_str())?;
        let accepted_pending = server.accept().await?;
        send_hello(&mut accepted_client, &hello(0)?).await?;

        let mut rejected_client = ClientOptions::new().open(server.endpoint().as_os_str())?;
        let rejected_pending = server.accept().await?;
        send_hello(&mut rejected_client, &hello(1)?).await?;

        let SessionAcceptance::Established(mut established) = accepted_pending.negotiate().await?
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
        let expected_invocation = invocation(established.manager_epoch())?;
        let expected_invocation_id = expected_invocation.invocation_id();
        send_invocation(&mut accepted_client, &expected_invocation).await?;
        let request = established.receive_request().await?;
        assert_eq!(
            request.authority().principal(),
            established.peer().principal_id()
        );
        assert!(matches!(request.stream_id(), StreamId::ClientInitiated(_)));
        assert_eq!(
            request.into_request(),
            SessionRequest::Invoke(expected_invocation)
        );
        let lease_request = InvocationLeaseRequest::new(None, NonZeroU32::MIN.saturating_add(31));
        send_lease_request(&mut accepted_client, lease_request).await?;
        let request = established.receive_request().await?;
        let lease_reply_target = request.reply_target();
        assert_eq!(
            request.into_request(),
            SessionRequest::LeaseInvocationIds(lease_request)
        );
        let lease_reply = InvocationLeaseReply::Rejected(InvocationLeaseRejection::CapacityFull);
        let mut other_connection = established.connection_id().into_bytes();
        other_connection[0] ^= u8::MAX;
        let wrong_target = ReplyTarget {
            connection_id: ConnectionId::new(other_connection)?,
            stream_id: lease_reply_target.stream_id(),
        };
        assert!(matches!(
            established
                .send_reply(wrong_target, SessionReply::InvocationLease(lease_reply))
                .await,
            Err(TransportError::WrongReplyConnection { .. })
        ));
        established
            .send_reply(
                lease_reply_target,
                SessionReply::InvocationLease(lease_reply),
            )
            .await?;
        let reply = receive_frame(&mut accepted_client).await?;
        assert_eq!(reply.header().kind(), INVOCATION_LEASE_REPLY_FRAME_KIND);
        assert_eq!(reply.header().stream_id(), lease_reply_target.stream_id());
        assert_eq!(
            InvocationLeaseCodec::decode_reply(reply.payload())?,
            lease_reply
        );

        let status_request = InvocationStatusRequest::new(expected_invocation_id);
        send_status_request(&mut accepted_client, status_request).await?;
        let request = established.receive_request().await?;
        let status_reply_target = request.reply_target();
        assert_eq!(
            request.into_request(),
            SessionRequest::InvocationStatus(status_request)
        );
        let status_reply =
            InvocationStatusReply::Unavailable(InvocationUnavailable::UnknownInvocation);
        established
            .send_reply(
                status_reply_target,
                SessionReply::InvocationStatus(status_reply),
            )
            .await?;
        let reply = receive_frame(&mut accepted_client).await?;
        assert_eq!(reply.header().kind(), INVOCATION_STATUS_REPLY_FRAME_KIND);
        assert_eq!(reply.header().stream_id(), status_reply_target.stream_id());
        assert_eq!(
            InvocationControlCodec::decode_status_reply(reply.payload())?,
            status_reply
        );

        let cancel_request = CancelInvocationRequest::new(expected_invocation_id);
        send_cancel_request(&mut accepted_client, cancel_request).await?;
        let request = established.receive_request().await?;
        let cancel_reply_target = request.reply_target();
        assert_eq!(
            request.into_request(),
            SessionRequest::CancelInvocation(cancel_request)
        );
        let cancel_reply =
            CancelInvocationReply::Unavailable(InvocationUnavailable::UnknownInvocation);
        established
            .send_reply(
                cancel_reply_target,
                SessionReply::CancelInvocation(cancel_reply),
            )
            .await?;
        let reply = receive_frame(&mut accepted_client).await?;
        assert_eq!(reply.header().kind(), CANCEL_INVOCATION_REPLY_FRAME_KIND);
        assert_eq!(reply.header().stream_id(), cancel_reply_target.stream_id());
        assert_eq!(
            InvocationControlCodec::decode_cancel_reply(reply.payload())?,
            cancel_reply
        );

        let revision = Revision::try_from(11)?;
        let catalog_stamp =
            CatalogStamp::new(established.manager_epoch(), revision, revision, revision);
        let catalog_query = CatalogQuery::new(Some(catalog_stamp));
        send_catalog_query(&mut accepted_client, catalog_query).await?;
        let request = established.receive_request().await?;
        let catalog_reply_target = request.reply_target();
        assert_eq!(
            request.into_request(),
            SessionRequest::GetCatalog(catalog_query)
        );
        let catalog_reply = CatalogReply::NotModified(catalog_stamp);
        established
            .send_reply(
                catalog_reply_target,
                SessionReply::Catalog(catalog_reply.clone()),
            )
            .await?;
        let reply = receive_frame(&mut accepted_client).await?;
        assert_eq!(reply.header().kind(), CATALOG_REPLY_FRAME_KIND);
        assert_eq!(reply.header().stream_id(), catalog_reply_target.stream_id());
        let mut reassembler = CatalogReassembler::new(established.negotiated().limits());
        assert_eq!(reassembler.push(reply.payload())?, Some(catalog_reply));

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
