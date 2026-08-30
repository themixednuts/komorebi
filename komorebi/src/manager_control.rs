use std::num::NonZeroUsize;

use crossbeam_channel::Receiver;
use crossbeam_channel::Sender;
use crossbeam_channel::TrySendError;
use komorebi_protocol::ActionInvocation;
use komorebi_protocol::AuthoritySummary;
use komorebi_protocol::CatalogQuery;
use komorebi_protocol::CatalogReply;
use komorebi_protocol::CommandCapability;
use komorebi_protocol::CommandCodecError;
use komorebi_protocol::InvocationRejection;
use komorebi_protocol::InvocationSubmissionReply;
use komorebi_protocol::PrincipalId;
use thiserror::Error;
use tokio::sync::oneshot;

use crate::action::PreparedCommitError;
use crate::adapters::action_catalog::action_grants;
use crate::command_control::CommandControlError;
use crate::command_control::ManagerInvocationLedger;
use crate::window_manager::CatalogReplyError;
use crate::window_manager::WindowManager;

mod invocation;

pub use invocation::InvocationDocumentError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ManagerControlCapacity(NonZeroUsize);

impl ManagerControlCapacity {
    pub const DEFAULT: Self = Self(match NonZeroUsize::new(128) {
        Some(value) => value,
        None => unreachable!(),
    });

    #[must_use]
    pub const fn new(value: NonZeroUsize) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> usize {
        self.0.get()
    }
}

#[derive(Clone)]
pub struct ManagerControl {
    sender: Sender<ManagerControlRequest>,
}

pub struct ManagerControlReceiver {
    receiver: Receiver<ManagerControlRequest>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub(crate) enum ManagerControlFlow {
    Continue,
    StopForRecovery,
}

pub(crate) enum ManagerControlRequest {
    GetCatalog {
        authority: AuthoritySummary,
        query: CatalogQuery,
        reply: oneshot::Sender<Result<CatalogReply, CatalogReplyError>>,
    },
    Invoke {
        principal: PrincipalId,
        authority: AuthoritySummary,
        invocation: ActionInvocation,
        reply: oneshot::Sender<Result<InvocationSubmissionReply, ManagerControlError>>,
    },
}

impl ManagerControl {
    #[must_use]
    pub fn channel(capacity: ManagerControlCapacity) -> (Self, ManagerControlReceiver) {
        let (sender, receiver) = crossbeam_channel::bounded(capacity.get());
        (Self { sender }, ManagerControlReceiver { receiver })
    }

    /// Requests one exact authority-scoped catalog from the manager thread.
    ///
    /// The bounded ingress is admitted without blocking a Tokio worker. Once
    /// admitted, cancellation only drops the reply receiver; the read-only
    /// manager request remains safe to complete.
    ///
    /// # Errors
    ///
    /// Returns [`ManagerControlError`] if the caller lacks catalog authority,
    /// ingress is saturated or closed, the reply is dropped, or catalog
    /// observation/projection fails.
    pub async fn catalog(
        &self,
        authority: AuthoritySummary,
        query: CatalogQuery,
    ) -> Result<CatalogReply, ManagerControlError> {
        if !authority.permits(CommandCapability::ReadCatalog) {
            return Err(ManagerControlError::UnauthorizedCatalog);
        }

        let (reply, response) = oneshot::channel();
        let request = ManagerControlRequest::GetCatalog {
            authority,
            query,
            reply,
        };
        match self.sender.try_send(request) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => return Err(ManagerControlError::Saturated),
            Err(TrySendError::Disconnected(_)) => return Err(ManagerControlError::Closed),
        }

        response
            .await
            .map_err(|_| ManagerControlError::ReplyDropped)?
            .map_err(ManagerControlError::Catalog)
    }

    /// Submits one authenticated canonical invocation to the serialized
    /// manager owner.
    ///
    /// # Errors
    ///
    /// Returns [`ManagerControlError`] when authority is missing, bounded
    /// ingress cannot admit the request, the reply is dropped, or durable
    /// manager execution fails.
    pub async fn invoke(
        &self,
        principal: PrincipalId,
        authority: AuthoritySummary,
        invocation: ActionInvocation,
    ) -> Result<InvocationSubmissionReply, ManagerControlError> {
        if !authority.permits(CommandCapability::InvokeActions) {
            return Ok(InvocationSubmissionReply::Rejected(
                InvocationRejection::Unauthorized,
            ));
        }

        let (reply, response) = oneshot::channel();
        self.try_send(ManagerControlRequest::Invoke {
            principal,
            authority,
            invocation,
            reply,
        })?;
        response
            .await
            .map_err(|_| ManagerControlError::ReplyDropped)?
    }

    fn try_send(&self, request: ManagerControlRequest) -> Result<(), ManagerControlError> {
        match self.sender.try_send(request) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => Err(ManagerControlError::Saturated),
            Err(TrySendError::Disconnected(_)) => Err(ManagerControlError::Closed),
        }
    }
}

impl ManagerControlReceiver {
    pub(crate) const fn receiver(&self) -> &Receiver<ManagerControlRequest> {
        &self.receiver
    }

    pub(crate) fn handle(
        &self,
        manager: &mut WindowManager,
        ledger: &ManagerInvocationLedger,
        request: ManagerControlRequest,
    ) -> ManagerControlFlow {
        match request {
            ManagerControlRequest::GetCatalog {
                authority,
                query,
                reply,
            } => {
                let result = manager.action_catalog_reply(action_grants(&authority), query.known());
                let _ = reply.send(result);
                ManagerControlFlow::Continue
            }
            ManagerControlRequest::Invoke {
                principal,
                authority,
                invocation,
                reply,
            } => {
                let result = invocation::submit(manager, ledger, principal, authority, invocation);
                let flow = if result
                    .as_ref()
                    .is_err_and(ManagerControlError::requires_recovery)
                {
                    ManagerControlFlow::StopForRecovery
                } else {
                    ManagerControlFlow::Continue
                };
                let _ = reply.send(result);
                flow
            }
        }
    }
}

#[derive(Debug, Error)]
pub enum ManagerControlError {
    #[error("principal is not authorized to read the action catalog")]
    UnauthorizedCatalog,
    #[error("manager control ingress is saturated")]
    Saturated,
    #[error("manager control ingress is closed")]
    Closed,
    #[error("manager control reply was dropped")]
    ReplyDropped,
    #[error("manager catalog request failed: {0}")]
    Catalog(#[source] CatalogReplyError),
    #[error("manager observation failed during invocation admission: {0}")]
    Observation(#[from] komorebi_protocol::ActionContractError),
    #[error("action catalog projection failed during invocation admission: {0}")]
    Projection(#[from] crate::adapters::action_catalog::CatalogProjectionError),
    #[error("canonical invocation contract verification failed: {0}")]
    InvocationCodec(#[from] CommandCodecError),
    #[error("command ledger request failed: {0}")]
    Ledger(#[from] CommandControlError),
    #[error("manager publication failed after its durable logical commit: {0}")]
    PostCommitManager(#[source] PreparedCommitError),
    #[error("durable ledger failed while attempting to {stage} after manager commit: {source}")]
    PostCommitLedger {
        stage: &'static str,
        #[source]
        source: CommandControlError,
    },
    #[error("terminal outcome encoding failed after native dispatch: {0}")]
    PostCommitDocument(#[source] InvocationDocumentError),
    #[error("the manager rejected an action after its durable logical commit")]
    CommittedAdmissionRejected,
    #[error(
        "durable logical state {durable:?} differs from manager state {manager:?} after commit"
    )]
    CommittedStateMismatch {
        durable: komorebi_protocol::StateStamp,
        manager: komorebi_protocol::StateStamp,
    },
    #[error("effect-dispatch transition was not applied after manager commit")]
    DispatchTransition,
    #[error("terminal transition was not applied after native dispatch")]
    TerminalTransition,
}

impl ManagerControlError {
    const fn requires_recovery(&self) -> bool {
        matches!(
            self,
            Self::PostCommitManager(_)
                | Self::PostCommitLedger { .. }
                | Self::PostCommitDocument(_)
                | Self::CommittedAdmissionRejected
                | Self::CommittedStateMismatch { .. }
                | Self::DispatchTransition
                | Self::TerminalTransition
        )
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use komorebi_protocol::ActionArguments;
    use komorebi_protocol::ActionInvocation;
    use komorebi_protocol::CatalogReply;
    use komorebi_protocol::InvocationId;
    use komorebi_protocol::InvocationLeaseReply;
    use komorebi_protocol::InvocationLeaseRequest;
    use komorebi_protocol::InvocationProgress;
    use komorebi_protocol::InvocationSequence;
    use komorebi_protocol::InvocationStatusReply;
    use komorebi_protocol::InvocationStatusRequest;
    use komorebi_protocol::InvocationTerminal;
    use komorebi_protocol::ManagerEpoch;
    use komorebi_protocol::PrincipalId;
    use komorebi_protocol::SettledInvocationKind;

    use super::*;

    #[tokio::test]
    async fn catalog_request_crosses_one_bounded_typed_ingress() {
        let (control, receiver) = ManagerControl::channel(ManagerControlCapacity::DEFAULT);
        let directory = tempfile::tempdir().expect("test directory should be created");
        let (_commands, ledger, ledger_owner) = crate::command_control::CommandControlPlane::start(
            directory.path().join("commands.sqlite"),
        )
        .await
        .expect("test ledger should start");
        let owner = std::thread::spawn(move || {
            let mut manager = WindowManager::new(
                ManagerEpoch::new([7; 16]).expect("test manager epoch is non-nil"),
            )
            .expect("test manager should initialize");
            let request = receiver
                .receiver()
                .recv()
                .expect("test control sender should remain connected");
            let _ = receiver.handle(&mut manager, &ledger, request);
        });

        let reply = control
            .catalog(AuthoritySummary::command_owner(), CatalogQuery::new(None))
            .await
            .expect("authorized catalog request should succeed");

        assert!(matches!(reply, CatalogReply::Snapshot(_)));
        owner.join().expect("test manager thread should stop");
        drop(control);
        ledger_owner
            .shutdown()
            .await
            .expect("test ledger should stop");
    }

    #[tokio::test]
    async fn catalog_authority_is_rejected_before_ingress() {
        let (control, _receiver) = ManagerControl::channel(ManagerControlCapacity::DEFAULT);

        let error = control
            .catalog(AuthoritySummary::default(), CatalogQuery::new(None))
            .await
            .expect_err("missing read authority should be rejected");

        assert!(matches!(error, ManagerControlError::UnauthorizedCatalog));
    }

    #[tokio::test]
    async fn canonical_invocation_crosses_durable_commit_and_native_dispatch()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let (commands, ledger, ledger_owner) = crate::command_control::CommandControlPlane::start(
            directory.path().join("commands.sqlite"),
        )
        .await?;
        let (manager, receiver) = ManagerControl::channel(ManagerControlCapacity::DEFAULT);
        let manager_owner = std::thread::spawn(move || -> Result<WindowManager, String> {
            let mut window_manager =
                WindowManager::new(ManagerEpoch::new([7; 16]).map_err(|error| error.to_string())?)
                    .map_err(|error| error.to_string())?;
            for _ in 0..5 {
                let request = receiver
                    .receiver()
                    .recv()
                    .map_err(|error| error.to_string())?;
                let _ = receiver.handle(&mut window_manager, &ledger, request);
            }
            Ok(window_manager)
        });
        let principal = PrincipalId::new([3; 32])?;
        let lease = match commands
            .lease(
                principal,
                InvocationLeaseRequest::new(None, NonZeroU32::MIN),
            )
            .await?
        {
            InvocationLeaseReply::Issued(lease) => lease,
            InvocationLeaseReply::Rejected(reason) => {
                return Err(format!("test invocation lease rejected: {reason:?}").into());
            }
        };
        let catalog = match manager
            .catalog(AuthoritySummary::command_owner(), CatalogQuery::new(None))
            .await?
        {
            CatalogReply::Snapshot(catalog) => catalog,
            CatalogReply::NotModified(_) => return Err("initial catalog was not modified".into()),
        };
        let offer = catalog
            .offers()
            .iter()
            .find(|offer| offer.reference().action().id().as_str() == "toggle-pause")
            .ok_or("toggle-pause offer missing")?;
        let invocation_id = InvocationId::new(lease.namespace(), lease.first());
        let invocation = ActionInvocation::new(
            invocation_id,
            offer.reference().clone(),
            catalog.state(),
            ActionArguments::default(),
            None,
        );

        let reply = manager
            .invoke(
                principal,
                AuthoritySummary::command_owner(),
                invocation.clone(),
            )
            .await?;
        let InvocationSubmissionReply::Accepted(status) = reply else {
            return Err(format!("canonical invocation was not accepted: {reply:?}").into());
        };
        assert_eq!(status.invocation_id(), invocation_id);
        assert!(matches!(
            status.progress(),
            InvocationProgress::Terminal(InvocationTerminal::Settled {
                kind: SettledInvocationKind::Succeeded,
                ..
            })
        ));
        assert_eq!(
            manager
                .invoke(principal, AuthoritySummary::command_owner(), invocation)
                .await?,
            InvocationSubmissionReply::Retained(status)
        );
        assert_eq!(
            commands
                .status(principal, InvocationStatusRequest::new(invocation_id))
                .await?,
            InvocationStatusReply::Retained(status)
        );

        let refreshed = match manager
            .catalog(AuthoritySummary::command_owner(), CatalogQuery::new(None))
            .await?
        {
            CatalogReply::Snapshot(catalog) => catalog,
            CatalogReply::NotModified(_) => return Err("advanced catalog was not modified".into()),
        };
        let refreshed_offer = refreshed
            .offers()
            .iter()
            .find(|offer| offer.reference().action().id().as_str() == "toggle-pause")
            .ok_or("refreshed toggle-pause offer missing")?;
        let unleased = ActionInvocation::new(
            InvocationId::new(lease.namespace(), InvocationSequence::try_from(2)?),
            refreshed_offer.reference().clone(),
            refreshed.state(),
            ActionArguments::default(),
            None,
        );
        assert_eq!(
            manager
                .invoke(principal, AuthoritySummary::command_owner(), unleased)
                .await?,
            InvocationSubmissionReply::Rejected(InvocationRejection::InvocationNotLeased)
        );

        let window_manager = manager_owner
            .join()
            .map_err(|_| "test manager owner panicked")?
            .map_err(|error| format!("test manager owner failed: {error}"))?;
        assert!(window_manager.is_paused);
        drop(manager);
        drop(commands);
        ledger_owner.shutdown().await?;
        Ok(())
    }
}
