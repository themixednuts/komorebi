use std::num::NonZeroU32;
use std::time::Duration;

use komorebi_protocol::ActionInvocation;
use komorebi_protocol::ActionInvocationCodec;
use komorebi_protocol::BootstrapCodec;
use komorebi_protocol::CANCEL_INVOCATION_FRAME_KIND;
use komorebi_protocol::CANCEL_INVOCATION_REPLY_FRAME_KIND;
use komorebi_protocol::CATALOG_REPLY_FRAME_KIND;
use komorebi_protocol::CancelInvocationReply;
use komorebi_protocol::CancelInvocationRequest;
use komorebi_protocol::CatalogCodec;
use komorebi_protocol::CatalogQuery;
use komorebi_protocol::CatalogReassembler;
use komorebi_protocol::CatalogReply;
use komorebi_protocol::Frame;
use komorebi_protocol::FrameKind;
use komorebi_protocol::GET_CATALOG_FRAME_KIND;
use komorebi_protocol::HELLO_FRAME_KIND;
use komorebi_protocol::Hello;
use komorebi_protocol::INVOCATION_LEASE_REPLY_FRAME_KIND;
use komorebi_protocol::INVOCATION_STATUS_FRAME_KIND;
use komorebi_protocol::INVOCATION_STATUS_REPLY_FRAME_KIND;
use komorebi_protocol::INVOKE_ACTION_FRAME_KIND;
use komorebi_protocol::INVOKE_ACTION_REPLY_FRAME_KIND;
use komorebi_protocol::InvocationControlCodec;
use komorebi_protocol::InvocationLeaseCodec;
use komorebi_protocol::InvocationLeaseReply;
use komorebi_protocol::InvocationLeaseRequest;
use komorebi_protocol::InvocationStatusReply;
use komorebi_protocol::InvocationStatusRequest;
use komorebi_protocol::InvocationSubmissionCodec;
use komorebi_protocol::InvocationSubmissionReply;
use komorebi_protocol::LEASE_INVOCATION_IDS_FRAME_KIND;
use komorebi_protocol::PROTOCOL_FAULT_FRAME_KIND;
use komorebi_protocol::ProtocolNegotiator;
use komorebi_protocol::RoleHint;
use komorebi_protocol::ServerSupport;
use komorebi_protocol::StreamId;
use komorebi_protocol::UNSUPPORTED_VERSION_FRAME_KIND;
use komorebi_protocol::WELCOME_FRAME_KIND;
use komorebi_protocol::Welcome;
use tokio::net::windows::named_pipe::ClientOptions;

use crate::CommandPipeEndpoint;
use crate::TransportError;
use crate::pipe::FramedPipe;

/// A sequential, authenticated client session for the local command protocol.
///
/// A request owns the session until its reply has been completely validated.
/// If a request future is cancelled, the session remains poisoned instead of
/// guessing whether a partial write or reply completed; drop it and reconnect.
pub struct CommandProtocolClient {
    connection: FramedPipe<tokio::net::windows::named_pipe::NamedPipeClient>,
    welcome: Welcome,
    next_stream: Option<NonZeroU32>,
    request_in_progress: bool,
}

impl CommandProtocolClient {
    /// Connects to the current Windows session and completes protocol
    /// negotiation.
    ///
    /// The role hint is descriptive only. Windows-derived peer identity and
    /// server policy determine the authority returned in [`Welcome`].
    ///
    /// # Errors
    ///
    /// Returns [`TransportError`] for endpoint, pipe, framing, bootstrap, or
    /// negotiation failures.
    pub async fn connect_current(role_hint: RoleHint) -> Result<Self, TransportError> {
        let endpoint = CommandPipeEndpoint::current()?;
        let pipe = ClientOptions::new()
            .open(endpoint.as_os_str())
            .map_err(|error| TransportError::io("open command pipe", error))?;
        let mut connection = FramedPipe::awaiting_inbound_preface(pipe);
        let support = ServerSupport::v1();
        let hello = Hello::new(
            support.protocol_versions().clone(),
            support.catalog_schemas().clone(),
            support.features().clone(),
            Some(role_hint),
        );
        connection.queue_frame(
            HELLO_FRAME_KIND,
            StreamId::Control,
            BootstrapCodec::encode_hello(&hello)?,
        )?;
        connection.flush_queued_frame().await?;
        let frame = connection.receive_frame().await?;
        if frame.header().stream_id() != StreamId::Control {
            return Err(TransportError::BootstrapMustUseControlStream(
                frame.header().stream_id(),
            ));
        }
        let welcome = match frame.header().kind() {
            WELCOME_FRAME_KIND => BootstrapCodec::decode_welcome(frame.payload())?,
            UNSUPPORTED_VERSION_FRAME_KIND => {
                return Err(TransportError::UnsupportedVersion(
                    BootstrapCodec::decode_unsupported_version(frame.payload())?,
                ));
            }
            PROTOCOL_FAULT_FRAME_KIND => {
                let fault = BootstrapCodec::decode_protocol_fault(frame.payload())?;
                return Err(TransportError::ProtocolFault {
                    code: fault.code(),
                    trace_id: fault.trace_id(),
                });
            }
            actual => {
                return Err(TransportError::UnexpectedBootstrapFrame {
                    expected: WELCOME_FRAME_KIND,
                    actual,
                });
            }
        };
        let expected = ProtocolNegotiator::select(&support, &hello)
            .map_err(|_| TransportError::NegotiationMismatch)?;
        if welcome.negotiated() != &expected {
            return Err(TransportError::NegotiationMismatch);
        }

        Ok(Self {
            connection,
            welcome,
            next_stream: Some(NonZeroU32::MIN),
            request_in_progress: false,
        })
    }

    #[must_use]
    pub const fn welcome(&self) -> &Welcome {
        &self.welcome
    }

    /// Leases a bounded range of durable invocation identities.
    ///
    /// # Errors
    ///
    /// Returns [`TransportError`] if the session is busy or the exchange fails.
    pub async fn lease_invocation_ids(
        &mut self,
        request: InvocationLeaseRequest,
    ) -> Result<InvocationLeaseReply, TransportError> {
        let frame = self
            .exchange_single(
                LEASE_INVOCATION_IDS_FRAME_KIND,
                InvocationLeaseCodec::encode_request(request)?,
                INVOCATION_LEASE_REPLY_FRAME_KIND,
            )
            .await?;
        let reply = InvocationLeaseCodec::decode_reply(frame.payload())?;
        self.request_in_progress = false;
        Ok(reply)
    }

    /// Reads one principal-scoped durable invocation status.
    ///
    /// # Errors
    ///
    /// Returns [`TransportError`] if the session is busy or the exchange fails.
    pub async fn invocation_status(
        &mut self,
        request: InvocationStatusRequest,
    ) -> Result<InvocationStatusReply, TransportError> {
        let frame = self
            .exchange_single(
                INVOCATION_STATUS_FRAME_KIND,
                InvocationControlCodec::encode_status_request(request)?,
                INVOCATION_STATUS_REPLY_FRAME_KIND,
            )
            .await?;
        let reply = InvocationControlCodec::decode_status_reply(frame.payload())?;
        self.request_in_progress = false;
        Ok(reply)
    }

    /// Requests cancellation of one uncommitted invocation.
    ///
    /// # Errors
    ///
    /// Returns [`TransportError`] if the session is busy or the exchange fails.
    pub async fn cancel_invocation(
        &mut self,
        request: CancelInvocationRequest,
    ) -> Result<CancelInvocationReply, TransportError> {
        let frame = self
            .exchange_single(
                CANCEL_INVOCATION_FRAME_KIND,
                InvocationControlCodec::encode_cancel_request(request)?,
                CANCEL_INVOCATION_REPLY_FRAME_KIND,
            )
            .await?;
        let reply = InvocationControlCodec::decode_cancel_reply(frame.payload())?;
        self.request_in_progress = false;
        Ok(reply)
    }

    /// Projects the authorized action catalog, reassembling a bounded chunked
    /// reply under the negotiated deadline.
    ///
    /// # Errors
    ///
    /// Returns [`TransportError`] if the session is busy, the deadline expires,
    /// or any chunk or catalog invariant fails.
    pub async fn catalog(&mut self, query: CatalogQuery) -> Result<CatalogReply, TransportError> {
        let stream =
            self.begin_request(GET_CATALOG_FRAME_KIND, CatalogCodec::encode_query(query)?)?;
        self.connection.flush_queued_frame().await?;
        let limits = self.welcome.negotiated().limits();
        let deadline = Duration::from_millis(u64::from(limits.assembly_deadline().get()));
        let mut reassembler = CatalogReassembler::new(limits);
        let reply = tokio::time::timeout(deadline, async {
            loop {
                let frame = self.connection.receive_frame().await?;
                validate_reply(&frame, stream, CATALOG_REPLY_FRAME_KIND)?;
                if let Some(reply) = reassembler.push(frame.payload())? {
                    return Ok::<CatalogReply, TransportError>(reply);
                }
            }
        })
        .await
        .map_err(|_| TransportError::CatalogAssemblyDeadline)??;
        self.request_in_progress = false;
        Ok(reply)
    }

    /// Submits one fully bound catalog invocation.
    ///
    /// # Errors
    ///
    /// Returns [`TransportError`] if the session is busy or the exchange fails.
    pub async fn invoke(
        &mut self,
        invocation: &ActionInvocation,
    ) -> Result<InvocationSubmissionReply, TransportError> {
        let frame = self
            .exchange_single(
                INVOKE_ACTION_FRAME_KIND,
                ActionInvocationCodec::encode(invocation)?,
                INVOKE_ACTION_REPLY_FRAME_KIND,
            )
            .await?;
        let reply = InvocationSubmissionCodec::decode(frame.payload())?;
        self.request_in_progress = false;
        Ok(reply)
    }

    async fn exchange_single(
        &mut self,
        request_kind: FrameKind,
        payload: Vec<u8>,
        reply_kind: FrameKind,
    ) -> Result<Frame, TransportError> {
        let stream = self.begin_request(request_kind, payload)?;
        self.connection.flush_queued_frame().await?;
        let frame = self.connection.receive_frame().await?;
        validate_reply(&frame, stream, reply_kind)?;
        Ok(frame)
    }

    fn begin_request(
        &mut self,
        kind: FrameKind,
        payload: Vec<u8>,
    ) -> Result<StreamId, TransportError> {
        if self.request_in_progress {
            return Err(TransportError::ClientRequestInProgress);
        }
        let value = self
            .next_stream
            .take()
            .ok_or(TransportError::ClientStreamsExhausted)?;
        let stream = StreamId::client_initiated(value)?;
        self.next_stream = value.get().checked_add(2).and_then(NonZeroU32::new);
        self.request_in_progress = true;
        self.connection.queue_frame(kind, stream, payload)?;
        Ok(stream)
    }
}

fn validate_reply(
    frame: &Frame,
    expected_stream: StreamId,
    expected_kind: FrameKind,
) -> Result<(), TransportError> {
    let actual_stream = frame.header().stream_id();
    if actual_stream != expected_stream {
        return Err(TransportError::UnexpectedReplyStream {
            expected: expected_stream,
            actual: actual_stream,
        });
    }
    let actual_kind = frame.header().kind();
    if actual_kind != expected_kind {
        return Err(TransportError::UnexpectedReplyFrame {
            expected: expected_kind,
            actual: actual_kind,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU16;

    use komorebi_protocol::ActionArguments;
    use komorebi_protocol::ActionContractFingerprint;
    use komorebi_protocol::ActionId;
    use komorebi_protocol::ActionKey;
    use komorebi_protocol::ActionSchemaVersion;
    use komorebi_protocol::AuthoritySummary;
    use komorebi_protocol::CatalogStamp;
    use komorebi_protocol::InvocationId;
    use komorebi_protocol::InvocationLease;
    use komorebi_protocol::InvocationNamespaceId;
    use komorebi_protocol::InvocationRejection;
    use komorebi_protocol::InvocationSequence;
    use komorebi_protocol::ManagerEpoch;
    use komorebi_protocol::OfferRef;
    use komorebi_protocol::Revision;
    use komorebi_protocol::StateStamp;

    use super::*;
    use crate::CommandProtocolServer;
    use crate::SessionAcceptance;
    use crate::SessionReply;
    use crate::SessionRequest;

    #[tokio::test(flavor = "current_thread")]
    #[serial_test::serial]
    async fn production_client_negotiates_and_exchanges_typed_requests()
    -> Result<(), Box<dyn std::error::Error>> {
        let epoch = ManagerEpoch::new([1; 16])?;
        let revision = Revision::try_from(1)?;
        let stamp = CatalogStamp::new(epoch, revision, revision, revision);
        let namespace = InvocationNamespaceId::new([2; 16])?;
        let sequence = InvocationSequence::try_from(1)?;
        let lease = InvocationLease::new(namespace, sequence, NonZeroU32::MIN, sequence);
        let invocation = ActionInvocation::new(
            InvocationId::new(namespace, sequence),
            OfferRef::new(
                ActionKey::new(
                    ActionId::parse("test-action")?,
                    ActionSchemaVersion::new(NonZeroU16::MIN),
                ),
                ActionContractFingerprint::new([3; 32]),
                stamp,
            ),
            StateStamp::new(epoch, revision),
            ActionArguments::default(),
            None,
        );
        let expected_invocation = invocation.clone();
        let mut server = CommandProtocolServer::bind_current(
            epoch,
            ServerSupport::v1(),
            AuthoritySummary::command_owner(),
        )?;
        let server_task = tokio::spawn(async move {
            let pending = server.accept().await?;
            let SessionAcceptance::Established(mut session) = pending.negotiate().await? else {
                return Err(TransportError::NegotiationMismatch);
            };

            let request = session.receive_request().await?;
            let target = request.reply_target();
            assert_eq!(
                request.into_request(),
                SessionRequest::LeaseInvocationIds(InvocationLeaseRequest::new(
                    None,
                    NonZeroU32::MIN,
                ))
            );
            session
                .send_reply(
                    target,
                    SessionReply::InvocationLease(InvocationLeaseReply::Issued(lease)),
                )
                .await?;

            let request = session.receive_request().await?;
            let target = request.reply_target();
            assert_eq!(
                request.into_request(),
                SessionRequest::GetCatalog(CatalogQuery::new(Some(stamp)))
            );
            session
                .send_reply(
                    target,
                    SessionReply::Catalog(CatalogReply::NotModified(stamp)),
                )
                .await?;

            let request = session.receive_request().await?;
            let target = request.reply_target();
            assert_eq!(
                request.into_request(),
                SessionRequest::Invoke(expected_invocation)
            );
            session
                .send_reply(
                    target,
                    SessionReply::InvocationSubmission(InvocationSubmissionReply::Rejected(
                        InvocationRejection::Unauthorized,
                    )),
                )
                .await
        });

        let mut client = CommandProtocolClient::connect_current(RoleHint::OwnerControl).await?;
        assert_eq!(client.welcome().manager_epoch(), epoch);
        assert_eq!(
            client
                .lease_invocation_ids(InvocationLeaseRequest::new(None, NonZeroU32::MIN))
                .await?,
            InvocationLeaseReply::Issued(lease)
        );
        assert_eq!(
            client.catalog(CatalogQuery::new(Some(stamp))).await?,
            CatalogReply::NotModified(stamp)
        );
        assert_eq!(
            client.invoke(&invocation).await?,
            InvocationSubmissionReply::Rejected(InvocationRejection::Unauthorized)
        );
        server_task.await??;
        Ok(())
    }
}
