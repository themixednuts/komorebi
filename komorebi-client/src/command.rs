use std::num::NonZeroU32;

use crate::DefaultLayout;
use komorebi_command_transport::CommandProtocolClient;
use komorebi_command_transport::TransportError;
pub use komorebi_protocol::ActionArguments;
pub use komorebi_protocol::ActionId;
pub use komorebi_protocol::ActionKey;
pub use komorebi_protocol::BoundedText;
pub use komorebi_protocol::BuiltInActionId;
pub use komorebi_protocol::BuiltInAnimationPrefix;
pub use komorebi_protocol::BuiltInAnimationStyle;
pub use komorebi_protocol::BuiltInArgument;
pub use komorebi_protocol::BuiltInArguments;
pub use komorebi_protocol::BuiltInArgumentsError;
pub use komorebi_protocol::BuiltInAxis;
pub use komorebi_protocol::BuiltInBorderImplementation;
pub use komorebi_protocol::BuiltInBorderStyle;
pub use komorebi_protocol::BuiltInCursorWarpPolicy;
pub use komorebi_protocol::BuiltInCycle;
pub use komorebi_protocol::BuiltInDirection;
pub use komorebi_protocol::BuiltInHidingBehaviour;
pub use komorebi_protocol::BuiltInIdentifier;
pub use komorebi_protocol::BuiltInImplementation;
pub use komorebi_protocol::BuiltInLayout;
pub use komorebi_protocol::BuiltInMonocleBehaviour;
pub use komorebi_protocol::BuiltInMoveBehaviour;
pub use komorebi_protocol::BuiltInNamedAnimationStyle;
pub use komorebi_protocol::BuiltInNames;
pub use komorebi_protocol::BuiltInOperationBehaviour;
pub use komorebi_protocol::BuiltInParameterId;
pub use komorebi_protocol::BuiltInRatios;
pub use komorebi_protocol::BuiltInResizeStep;
pub use komorebi_protocol::BuiltInResizeStepError;
pub use komorebi_protocol::BuiltInSelector;
pub use komorebi_protocol::BuiltInSizing;
pub use komorebi_protocol::BuiltInStackbarLabel;
pub use komorebi_protocol::BuiltInStackbarMode;
pub use komorebi_protocol::BuiltInWindowKind;
pub use komorebi_protocol::BuiltInWorkspaceTarget;
use komorebi_protocol::CatalogQuery;
use komorebi_protocol::CatalogReply;
use komorebi_protocol::CatalogSnapshot;
pub use komorebi_protocol::FixedDecimal;
use komorebi_protocol::InvocationId;
use komorebi_protocol::InvocationIdentityError;
use komorebi_protocol::InvocationLease;
use komorebi_protocol::InvocationLeaseRejection;
use komorebi_protocol::InvocationLeaseReply;
use komorebi_protocol::InvocationLeaseRequest;
use komorebi_protocol::InvocationNamespaceId;
use komorebi_protocol::InvocationSequence;
pub use komorebi_protocol::InvocationSubmissionReply;
pub use komorebi_protocol::RoleHint;
pub use komorebi_protocol::WindowsPathInput;
use thiserror::Error;

/// Projects the public layout vocabulary onto its closed command-protocol shape.
#[must_use]
pub const fn built_in_layout(value: DefaultLayout) -> BuiltInLayout {
    match value {
        DefaultLayout::BSP => BuiltInLayout::Bsp,
        DefaultLayout::Columns => BuiltInLayout::Columns,
        DefaultLayout::Rows => BuiltInLayout::Rows,
        DefaultLayout::VerticalStack => BuiltInLayout::VerticalStack,
        DefaultLayout::HorizontalStack => BuiltInLayout::HorizontalStack,
        DefaultLayout::UltrawideVerticalStack => BuiltInLayout::UltrawideVerticalStack,
        DefaultLayout::Grid => BuiltInLayout::Grid,
        DefaultLayout::RightMainVerticalStack => BuiltInLayout::RightMainVerticalStack,
        DefaultLayout::Scrolling => BuiltInLayout::Scrolling,
    }
}

const PERSISTENT_INVOCATION_LEASE_SIZE: NonZeroU32 = NonZeroU32::MIN.saturating_add(255);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionLifetime {
    OneShot,
    Persistent,
}

impl SessionLifetime {
    const fn lease_size(self) -> NonZeroU32 {
        match self {
            Self::OneShot => NonZeroU32::MIN,
            Self::Persistent => PERSISTENT_INVOCATION_LEASE_SIZE,
        }
    }
}

/// A first-party command session with one exact catalog snapshot and a local
/// cursor over manager-issued durable invocation identities.
pub struct CommandClient {
    protocol: CommandProtocolClient,
    catalog: CatalogSnapshot,
    ids: InvocationIds,
    lifetime: SessionLifetime,
}

impl CommandClient {
    /// Connects, negotiates authority, leases invocation identities, and reads
    /// the initial authorized catalog.
    pub async fn connect(
        role_hint: RoleHint,
        lifetime: SessionLifetime,
    ) -> Result<Self, CommandClientError> {
        let mut protocol = CommandProtocolClient::connect_current(role_hint).await?;
        let lease = lease(&mut protocol, None, lifetime.lease_size()).await?;
        let catalog = match protocol.catalog(CatalogQuery::new(None)).await? {
            CatalogReply::Snapshot(catalog) => catalog,
            CatalogReply::NotModified(_) => return Err(CommandClientError::MissingInitialCatalog),
        };
        Ok(Self {
            protocol,
            catalog,
            ids: InvocationIds::new(lease),
            lifetime,
        })
    }

    #[must_use]
    pub const fn catalog(&self) -> &CatalogSnapshot {
        &self.catalog
    }

    /// Refreshes the authorized catalog only when its exact stamp changed.
    pub async fn refresh_catalog(&mut self) -> Result<(), CommandClientError> {
        match self
            .protocol
            .catalog(CatalogQuery::new(Some(self.catalog.stamp())))
            .await?
        {
            CatalogReply::Snapshot(catalog) => self.catalog = catalog,
            CatalogReply::NotModified(_) => {}
        }
        Ok(())
    }

    /// Invokes one exact action schema from the current catalog snapshot.
    ///
    /// Rejections are returned as protocol domain values and are never retried
    /// implicitly; callers decide whether refreshing and creating a new
    /// invocation matches the action's semantics.
    pub async fn invoke(
        &mut self,
        action: &ActionKey,
        arguments: ActionArguments,
    ) -> Result<InvocationSubmissionReply, CommandClientError> {
        let offer = self
            .catalog
            .offers()
            .binary_search_by(|offer| offer.reference().action().cmp(action))
            .ok()
            .and_then(|index| self.catalog.offers().get(index))
            .ok_or_else(|| CommandClientError::ActionNotOffered(action.clone()))?;
        let reference = offer.reference().clone();
        let expected_state = offer.state();
        let invocation_id = self.next_invocation_id().await?;
        let invocation = komorebi_protocol::ActionInvocation::new(
            invocation_id,
            reference,
            expected_state,
            arguments,
            None,
        );
        Ok(self.protocol.invoke(&invocation).await?)
    }

    /// Invokes the single currently offered schema for a stable action ID.
    pub async fn invoke_current(
        &mut self,
        action: &ActionId,
        arguments: ActionArguments,
    ) -> Result<InvocationSubmissionReply, CommandClientError> {
        let mut matches = self
            .catalog
            .offers()
            .iter()
            .filter(|offer| offer.reference().action().id() == action)
            .map(|offer| offer.reference().action().clone());
        let key = matches
            .next()
            .ok_or_else(|| CommandClientError::ActionIdNotOffered(action.clone()))?;
        if matches.next().is_some() {
            return Err(CommandClientError::AmbiguousActionId(action.clone()));
        }
        self.invoke(&key, arguments).await
    }

    /// Invokes one closed manager-owned built-in action identity.
    pub async fn invoke_builtin(
        &mut self,
        action: BuiltInActionId,
        arguments: ActionArguments,
    ) -> Result<InvocationSubmissionReply, CommandClientError> {
        self.invoke_current(&action.into_action_id(), arguments)
            .await
    }

    async fn next_invocation_id(&mut self) -> Result<InvocationId, CommandClientError> {
        if let Some(id) = self.ids.take()? {
            return Ok(id);
        }
        let lease = lease(
            &mut self.protocol,
            Some(self.ids.namespace()),
            self.lifetime.lease_size(),
        )
        .await?;
        self.ids = InvocationIds::new(lease);
        self.ids
            .take()?
            .ok_or(CommandClientError::EmptyInvocationLease)
    }
}

struct InvocationIds {
    namespace: InvocationNamespaceId,
    next: Option<LeaseCursor>,
}

impl InvocationIds {
    const fn new(lease: InvocationLease) -> Self {
        Self {
            namespace: lease.namespace(),
            next: Some(LeaseCursor {
                sequence: lease.first(),
                remaining: lease.count(),
            }),
        }
    }

    const fn namespace(&self) -> InvocationNamespaceId {
        self.namespace
    }

    fn take(&mut self) -> Result<Option<InvocationId>, InvocationIdentityError> {
        let Some(cursor) = self.next else {
            return Ok(None);
        };
        let id = InvocationId::new(self.namespace, cursor.sequence);
        self.next = match NonZeroU32::new(cursor.remaining.get() - 1) {
            Some(remaining) => Some(LeaseCursor {
                sequence: cursor.sequence.next()?,
                remaining,
            }),
            None => None,
        };
        Ok(Some(id))
    }
}

#[derive(Clone, Copy)]
struct LeaseCursor {
    sequence: InvocationSequence,
    remaining: NonZeroU32,
}

async fn lease(
    protocol: &mut CommandProtocolClient,
    namespace: Option<InvocationNamespaceId>,
    count: NonZeroU32,
) -> Result<InvocationLease, CommandClientError> {
    match protocol
        .lease_invocation_ids(InvocationLeaseRequest::new(namespace, count))
        .await?
    {
        InvocationLeaseReply::Issued(lease)
            if namespace.is_none_or(|id| id == lease.namespace()) =>
        {
            Ok(lease)
        }
        InvocationLeaseReply::Issued(_) => Err(CommandClientError::LeaseNamespaceChanged),
        InvocationLeaseReply::Rejected(reason) => Err(CommandClientError::LeaseRejected(reason)),
    }
}

#[derive(Debug, Error)]
pub enum CommandClientError {
    #[error("command transport failed: {0}")]
    Transport(#[from] TransportError),
    #[error("invocation identity allocation failed: {0}")]
    Identity(#[from] InvocationIdentityError),
    #[error("the manager rejected the invocation lease: {0:?}")]
    LeaseRejected(InvocationLeaseRejection),
    #[error("a renewed invocation lease changed its namespace")]
    LeaseNamespaceChanged,
    #[error("the initial catalog request returned NotModified without a local snapshot")]
    MissingInitialCatalog,
    #[error("the manager issued an empty invocation lease")]
    EmptyInvocationLease,
    #[error("the current catalog does not offer action {0:?}")]
    ActionNotOffered(ActionKey),
    #[error("the current catalog does not offer action ID {0}")]
    ActionIdNotOffered(ActionId),
    #[error("action ID {0} has more than one currently offered schema")]
    AmbiguousActionId(ActionId),
}

#[cfg(test)]
mod tests {
    use komorebi_protocol::InvocationSequence;

    use super::*;

    #[test]
    fn lease_cursor_allocates_every_identity_once() -> Result<(), Box<dyn std::error::Error>> {
        let namespace = InvocationNamespaceId::new([1; 16])?;
        let first = InvocationSequence::try_from(7)?;
        let mut ids = InvocationIds::new(InvocationLease::new(
            namespace,
            first,
            NonZeroU32::new(2).ok_or("nonzero test lease")?,
            first,
        ));

        assert_eq!(ids.take()?, Some(InvocationId::new(namespace, first)));
        assert_eq!(
            ids.take()?,
            Some(InvocationId::new(namespace, first.next()?))
        );
        assert_eq!(ids.take()?, None);
        Ok(())
    }
}
