use std::num::NonZeroU16;
use std::num::NonZeroU32;

use komorebi_command_transport::CommandProtocolServer;
use komorebi_command_transport::SessionAcceptance;
use komorebi_command_transport::SessionReply;
use komorebi_command_transport::SessionRequest;
use komorebi_command_transport::TransportError;
use komorebi_protocol::ActionArguments;
use komorebi_protocol::ActionAvailability;
use komorebi_protocol::ActionCategory;
use komorebi_protocol::ActionDefinition;
use komorebi_protocol::ActionDefinitionSpec;
use komorebi_protocol::ActionId;
use komorebi_protocol::ActionKey;
use komorebi_protocol::ActionOffer;
use komorebi_protocol::ActionSchemaVersion;
use komorebi_protocol::AuthoritySummary;
use komorebi_protocol::BoundedText;
use komorebi_protocol::BuiltInActionId;
use komorebi_protocol::CatalogCodec;
use komorebi_protocol::CatalogQuery;
use komorebi_protocol::CatalogReply;
use komorebi_protocol::CatalogSnapshot;
use komorebi_protocol::CatalogStamp;
use komorebi_protocol::ConfirmationPolicy;
use komorebi_protocol::InvocationId;
use komorebi_protocol::InvocationLease;
use komorebi_protocol::InvocationLeaseReply;
use komorebi_protocol::InvocationLeaseRequest;
use komorebi_protocol::InvocationNamespaceId;
use komorebi_protocol::InvocationRejection;
use komorebi_protocol::InvocationSequence;
use komorebi_protocol::InvocationSubmissionReply;
use komorebi_protocol::ManagerEpoch;
use komorebi_protocol::OfferRef;
use komorebi_protocol::PermittedUse;
use komorebi_protocol::Revision;
use komorebi_protocol::RoleHint;
use komorebi_protocol::ServerSupport;
use komorebi_protocol::StateStamp;
use komorebi_protocol::UndoPolicy;
use komorebi_shell::CommandPalette;
use komorebi_shell::PaletteActionState;
use komorebi_shell::PaletteQuery;
use komorebi_shell::PaletteResults;
use komorebi_shell::SessionLifetime;
use komorebi_shell::ShellSession;

fn catalog(epoch: ManagerEpoch) -> Result<CatalogSnapshot, Box<dyn std::error::Error>> {
    let revision = Revision::FIRST;
    let stamp = CatalogStamp::new(epoch, revision, revision, revision);
    let key = ActionKey::new(
        ActionId::parse(BuiltInActionId::TogglePause.as_str())?,
        ActionSchemaVersion::new(NonZeroU16::MIN),
    );
    let definition = ActionDefinition::new(ActionDefinitionSpec {
        key: key.clone(),
        category: ActionCategory::Configuration,
        title: BoundedText::new("Toggle pause")?,
        description: BoundedText::new("Toggle manager pause state")?,
        keywords: vec![],
        parameters: vec![],
        permitted_uses: vec![PermittedUse::Interactive],
        confirmation: ConfirmationPolicy::None,
        undo: UndoPolicy::None,
    })?;
    let fingerprint = CatalogCodec::definition_fingerprint(&definition)?;
    let offer = ActionOffer::new(
        OfferRef::new(key, fingerprint, stamp),
        StateStamp::new(epoch, revision),
        ActionAvailability::Available,
        None,
        vec![],
        vec![],
    )?;
    Ok(CatalogSnapshot::new(
        stamp,
        StateStamp::new(epoch, revision),
        vec![definition],
        vec![offer],
    )?)
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn dropped_ticket_does_not_cancel_or_poison_the_owned_session()
-> Result<(), Box<dyn std::error::Error>> {
    let epoch = ManagerEpoch::new([41; 16])?;
    let catalog = catalog(epoch)?;
    let catalog_stamp = catalog.stamp();
    let expected_reference = catalog.offers()[0].reference().clone();
    let expected_state = catalog.offers()[0].state();
    let namespace = InvocationNamespaceId::new([42; 16])?;
    let first = InvocationSequence::try_from(1)?;
    let lease_count = NonZeroU32::new(256).ok_or("test lease count must be nonzero")?;
    let lease = InvocationLease::new(namespace, first, lease_count, first);
    let mut server = CommandProtocolServer::bind_current(
        epoch,
        ServerSupport::v1(),
        AuthoritySummary::command_owner(),
    )?;
    let (server_ready, ready) = tokio::sync::oneshot::channel();
    let server_task = tokio::spawn(async move {
        if server_ready.send(()).is_err() {
            return Err(TransportError::NegotiationMismatch);
        }
        let pending = server.accept().await?;
        let SessionAcceptance::Established(mut session) = pending.negotiate().await? else {
            return Err(TransportError::NegotiationMismatch);
        };

        let request = session.receive_request().await?;
        let target = request.reply_target();
        assert_eq!(
            request.into_request(),
            SessionRequest::LeaseInvocationIds(InvocationLeaseRequest::new(None, lease_count))
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
            SessionRequest::GetCatalog(CatalogQuery::new(None))
        );
        session
            .send_reply(
                target,
                SessionReply::Catalog(CatalogReply::Snapshot(catalog)),
            )
            .await?;

        let request = session.receive_request().await?;
        let target = request.reply_target();
        assert_eq!(
            request.into_request(),
            SessionRequest::GetCatalog(CatalogQuery::new(Some(catalog_stamp)))
        );
        session
            .send_reply(
                target,
                SessionReply::Catalog(CatalogReply::NotModified(catalog_stamp)),
            )
            .await?;

        for sequence in [first, first.next()?] {
            let request = session.receive_request().await?;
            let target = request.reply_target();
            assert_eq!(
                request.into_request(),
                SessionRequest::GetCatalog(CatalogQuery::new(Some(catalog_stamp)))
            );
            session
                .send_reply(
                    target,
                    SessionReply::Catalog(CatalogReply::NotModified(catalog_stamp)),
                )
                .await?;

            let request = session.receive_request().await?;
            let target = request.reply_target();
            let SessionRequest::Invoke(invocation) = request.into_request() else {
                return Err(TransportError::NegotiationMismatch);
            };
            assert_eq!(
                invocation.invocation_id(),
                InvocationId::new(namespace, sequence)
            );
            assert_eq!(invocation.offer(), &expected_reference);
            assert_eq!(invocation.expected_state(), expected_state);
            assert_eq!(invocation.arguments(), &ActionArguments::default());
            session
                .send_reply(
                    target,
                    SessionReply::InvocationSubmission(InvocationSubmissionReply::Rejected(
                        InvocationRejection::Unauthorized,
                    )),
                )
                .await?;
        }
        Ok::<_, TransportError>(())
    });

    ready.await?;
    let session = ShellSession::start(RoleHint::OwnerControl, SessionLifetime::Persistent)?;
    let handle = session.handle();
    let observed_catalog = handle.catalog_snapshot()?.snapshot().await?;
    assert_eq!(observed_catalog.stamp(), catalog_stamp);
    let abandoned =
        handle.invoke_builtin(BuiltInActionId::TogglePause, ActionArguments::default())?;
    drop(abandoned);
    let palette = CommandPalette::project(&observed_catalog);
    let PaletteResults::Actions(matches) = palette.query(PaletteQuery::parse("pause")) else {
        return Err("pause query should search local actions".into());
    };
    let PaletteActionState::Ready(binding) = matches
        .selected(&palette)
        .ok_or("pause action should be searchable")?
        .state()
    else {
        return Err("pause action should be immediately invokable".into());
    };
    assert_eq!(
        handle.invoke_binding(binding)?.outcome().await?,
        InvocationSubmissionReply::Rejected(InvocationRejection::Unauthorized)
    );
    session.shutdown().await?;
    server_task.await??;
    Ok(())
}
