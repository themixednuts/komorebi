use std::num::NonZeroUsize;

use crossbeam_channel::Receiver;
use crossbeam_channel::Sender;
use crossbeam_channel::TrySendError;
use komorebi_protocol::AuthoritySummary;
use komorebi_protocol::CatalogQuery;
use komorebi_protocol::CatalogReply;
use komorebi_protocol::CommandCapability;
use thiserror::Error;
use tokio::sync::oneshot;

use crate::adapters::action_catalog::action_grants;
use crate::window_manager::CatalogReplyError;
use crate::window_manager::WindowManager;

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

pub(crate) enum ManagerControlRequest {
    GetCatalog {
        authority: AuthoritySummary,
        query: CatalogQuery,
        reply: oneshot::Sender<Result<CatalogReply, CatalogReplyError>>,
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
}

impl ManagerControlReceiver {
    pub(crate) const fn receiver(&self) -> &Receiver<ManagerControlRequest> {
        &self.receiver
    }

    pub(crate) fn handle(&self, manager: &mut WindowManager, request: ManagerControlRequest) {
        match request {
            ManagerControlRequest::GetCatalog {
                authority,
                query,
                reply,
            } => {
                let result = manager.action_catalog_reply(action_grants(&authority), query.known());
                let _ = reply.send(result);
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
}

#[cfg(test)]
mod tests {
    use komorebi_protocol::CatalogReply;
    use komorebi_protocol::ManagerEpoch;

    use super::*;

    #[tokio::test]
    async fn catalog_request_crosses_one_bounded_typed_ingress() {
        let (control, receiver) = ManagerControl::channel(ManagerControlCapacity::DEFAULT);
        let owner = std::thread::spawn(move || {
            let mut manager = WindowManager::new(
                ManagerEpoch::new([7; 16]).expect("test manager epoch is non-nil"),
            )
            .expect("test manager should initialize");
            let request = receiver
                .receiver()
                .recv()
                .expect("test control sender should remain connected");
            receiver.handle(&mut manager, request);
        });

        let reply = control
            .catalog(AuthoritySummary::command_owner(), CatalogQuery::new(None))
            .await
            .expect("authorized catalog request should succeed");

        assert!(matches!(reply, CatalogReply::Snapshot(_)));
        owner.join().expect("test manager thread should stop");
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
}
