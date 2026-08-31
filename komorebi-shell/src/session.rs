use std::num::NonZeroU32;

use komorebi_command_transport::CommandProtocolClient;
use komorebi_command_transport::TransportError;
use komorebi_protocol::ActionArguments;
use komorebi_protocol::ActionId;
use komorebi_protocol::ActionKey;
use komorebi_protocol::BuiltInActionId;
use komorebi_protocol::CatalogQuery;
use komorebi_protocol::CatalogReply;
use komorebi_protocol::CatalogSnapshot;
use komorebi_protocol::InvocationId;
use komorebi_protocol::InvocationIdentityError;
use komorebi_protocol::InvocationLease;
use komorebi_protocol::InvocationLeaseRejection;
use komorebi_protocol::InvocationLeaseReply;
use komorebi_protocol::InvocationLeaseRequest;
use komorebi_protocol::InvocationNamespaceId;
use komorebi_protocol::InvocationSequence;
use komorebi_protocol::InvocationSubmissionReply;
use komorebi_protocol::RoleHint;
use thiserror::Error;
use tokio::runtime::Handle;
use tokio::runtime::TryCurrentError;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::task::JoinError;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::ActionBinding;
use crate::ActionBindingError;

const PERSISTENT_INVOCATION_LEASE_SIZE: NonZeroU32 = NonZeroU32::MIN.saturating_add(255);
const COMMAND_QUEUE_CAPACITY: usize = 64;

/// Selects the amount of manager-issued invocation identity space retained by
/// one command session.
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

/// Owns one cancellation-safe command actor for a shell process.
pub struct ShellSession {
    dispatcher: ActionDispatcher,
    cancellation: CancellationToken,
    actor: JoinHandle<()>,
}

impl ShellSession {
    /// Starts a lazily connected command actor on the current Tokio runtime.
    pub fn start(
        role_hint: RoleHint,
        lifetime: SessionLifetime,
    ) -> Result<Self, ShellSessionStartError> {
        let runtime = Handle::try_current()?;
        let (sender, receiver) = mpsc::channel(COMMAND_QUEUE_CAPACITY);
        let cancellation = CancellationToken::new();
        let actor = runtime.spawn(run(receiver, cancellation.clone(), role_hint, lifetime));
        Ok(Self {
            dispatcher: ActionDispatcher { sender },
            cancellation,
            actor,
        })
    }

    #[must_use]
    pub fn dispatcher(&self) -> ActionDispatcher {
        self.dispatcher.clone()
    }

    /// Stops admission, lets the in-flight exchange finish, rejects queued
    /// work, and joins the actor.
    pub async fn shutdown(mut self) -> Result<(), ShellSessionShutdownError> {
        self.cancellation.cancel();
        (&mut self.actor).await?;
        Ok(())
    }
}

impl Drop for ShellSession {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}

/// A cloneable, nonblocking entrypoint to one owned shell session.
#[derive(Clone)]
pub struct ActionDispatcher {
    sender: mpsc::Sender<QueuedInvocation>,
}

impl ActionDispatcher {
    pub fn invoke_builtin(
        &self,
        action: BuiltInActionId,
        arguments: ActionArguments,
    ) -> Result<InvocationTicket, ActionDispatchError> {
        self.submit(RequestedAction::BuiltIn { action, arguments })
    }

    pub fn invoke_binding(
        &self,
        binding: ActionBinding,
    ) -> Result<InvocationTicket, ActionDispatchError> {
        self.submit(RequestedAction::Binding(binding))
    }

    fn submit(&self, action: RequestedAction) -> Result<InvocationTicket, ActionDispatchError> {
        let (outcome, receiver) = oneshot::channel();
        self.sender
            .try_send(QueuedInvocation { action, outcome })
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => ActionDispatchError::QueueFull,
                mpsc::error::TrySendError::Closed(_) => ActionDispatchError::SessionClosed,
            })?;
        Ok(InvocationTicket { receiver })
    }
}

/// Optional result interest for one accepted action submission.
pub struct InvocationTicket {
    receiver: oneshot::Receiver<Result<InvocationSubmissionReply, ActionInvocationError>>,
}

impl InvocationTicket {
    /// Waits for the command actor's complete submission outcome.
    pub async fn outcome(self) -> Result<InvocationSubmissionReply, ActionInvocationError> {
        match self.receiver.await {
            Ok(outcome) => outcome,
            Err(_) => Err(ActionInvocationError::SessionStopped),
        }
    }
}

struct QueuedInvocation {
    action: RequestedAction,
    outcome: oneshot::Sender<Result<InvocationSubmissionReply, ActionInvocationError>>,
}

enum RequestedAction {
    BuiltIn {
        action: BuiltInActionId,
        arguments: ActionArguments,
    },
    Binding(ActionBinding),
}

async fn run(
    mut receiver: mpsc::Receiver<QueuedInvocation>,
    cancellation: CancellationToken,
    role_hint: RoleHint,
    lifetime: SessionLifetime,
) {
    let mut connection = None;
    loop {
        tokio::select! {
            biased;
            () = cancellation.cancelled() => {
                receiver.close();
                while let Some(request) = receiver.recv().await {
                    drop(request.outcome.send(Err(ActionInvocationError::SessionShuttingDown)));
                }
                return;
            }
            request = receiver.recv() => {
                let Some(request) = request else {
                    return;
                };
                let outcome = dispatch(&mut connection, request.action, role_hint, lifetime).await;
                drop(request.outcome.send(outcome));
            }
        }
    }
}

async fn dispatch(
    current: &mut Option<CommandConnection>,
    action: RequestedAction,
    role_hint: RoleHint,
    lifetime: SessionLifetime,
) -> Result<InvocationSubmissionReply, ActionInvocationError> {
    let mut connection = match current.take() {
        Some(connection) => connection,
        None => CommandConnection::connect(role_hint, lifetime).await?,
    };
    let outcome = invoke(&mut connection, action).await;
    if !matches!(outcome, Err(ActionInvocationError::Session(_))) {
        *current = Some(connection);
    }
    outcome
}

async fn invoke(
    connection: &mut CommandConnection,
    action: RequestedAction,
) -> Result<InvocationSubmissionReply, ActionInvocationError> {
    connection.refresh_catalog().await?;
    Ok(match action {
        RequestedAction::BuiltIn { action, arguments } => {
            connection.invoke_builtin(action, arguments).await?
        }
        RequestedAction::Binding(binding) => {
            let bound = binding.bind(connection.catalog())?;
            let (action, arguments) = bound.into_parts();
            connection.invoke(&action, arguments).await?
        }
    })
}

/// A fully negotiated private connection with one exact catalog snapshot and
/// a local cursor over manager-issued durable invocation identities.
struct CommandConnection {
    protocol: CommandProtocolClient,
    catalog: CatalogSnapshot,
    ids: InvocationIds,
    lifetime: SessionLifetime,
}

impl CommandConnection {
    /// Connects, negotiates authority, leases invocation identities, and reads
    /// the initial authorized catalog.
    async fn connect(
        role_hint: RoleHint,
        lifetime: SessionLifetime,
    ) -> Result<Self, CommandSessionError> {
        let mut protocol = CommandProtocolClient::connect_current(role_hint).await?;
        let lease = lease(&mut protocol, None, lifetime.lease_size()).await?;
        let catalog = match protocol.catalog(CatalogQuery::new(None)).await? {
            CatalogReply::Snapshot(catalog) => catalog,
            CatalogReply::NotModified(_) => {
                return Err(CommandSessionError::MissingInitialCatalog);
            }
        };
        Ok(Self {
            protocol,
            catalog,
            ids: InvocationIds::new(lease),
            lifetime,
        })
    }

    #[must_use]
    const fn catalog(&self) -> &CatalogSnapshot {
        &self.catalog
    }

    /// Refreshes the authorized catalog only when its exact stamp changed.
    async fn refresh_catalog(&mut self) -> Result<(), CommandSessionError> {
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
    async fn invoke(
        &mut self,
        action: &ActionKey,
        arguments: ActionArguments,
    ) -> Result<InvocationSubmissionReply, CommandSessionError> {
        let offer = self
            .catalog
            .offers()
            .binary_search_by(|offer| offer.reference().action().cmp(action))
            .ok()
            .and_then(|index| self.catalog.offers().get(index))
            .ok_or_else(|| CommandSessionError::ActionNotOffered(action.clone()))?;
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
    async fn invoke_current(
        &mut self,
        action: &ActionId,
        arguments: ActionArguments,
    ) -> Result<InvocationSubmissionReply, CommandSessionError> {
        let mut matches = self
            .catalog
            .offers()
            .iter()
            .filter(|offer| offer.reference().action().id() == action)
            .map(|offer| offer.reference().action().clone());
        let key = matches
            .next()
            .ok_or_else(|| CommandSessionError::ActionIdNotOffered(action.clone()))?;
        if matches.next().is_some() {
            return Err(CommandSessionError::AmbiguousActionId(action.clone()));
        }
        self.invoke(&key, arguments).await
    }

    /// Invokes one closed manager-owned built-in action identity.
    async fn invoke_builtin(
        &mut self,
        action: BuiltInActionId,
        arguments: ActionArguments,
    ) -> Result<InvocationSubmissionReply, CommandSessionError> {
        self.invoke_current(&action.into_action_id(), arguments)
            .await
    }

    async fn next_invocation_id(&mut self) -> Result<InvocationId, CommandSessionError> {
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
            .ok_or(CommandSessionError::EmptyInvocationLease)
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
) -> Result<InvocationLease, CommandSessionError> {
    match protocol
        .lease_invocation_ids(InvocationLeaseRequest::new(namespace, count))
        .await?
    {
        InvocationLeaseReply::Issued(lease)
            if namespace.is_none_or(|id| id == lease.namespace()) =>
        {
            Ok(lease)
        }
        InvocationLeaseReply::Issued(_) => Err(CommandSessionError::LeaseNamespaceChanged),
        InvocationLeaseReply::Rejected(reason) => Err(CommandSessionError::LeaseRejected(reason)),
    }
}

#[derive(Debug, Error)]
pub enum ShellSessionStartError {
    #[error("a shell session requires an active Tokio runtime: {0}")]
    Runtime(#[from] TryCurrentError),
}

#[derive(Debug, Error)]
pub enum ShellSessionShutdownError {
    #[error("the shell session actor failed: {0}")]
    Actor(#[from] JoinError),
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum ActionDispatchError {
    #[error("the shell command queue is full")]
    QueueFull,
    #[error("the shell session is closed")]
    SessionClosed,
}

#[derive(Debug, Error)]
pub enum ActionInvocationError {
    #[error(transparent)]
    Session(#[from] CommandSessionError),
    #[error(transparent)]
    Binding(#[from] ActionBindingError),
    #[error("the shell session is shutting down")]
    SessionShuttingDown,
    #[error("the shell session stopped before reporting this invocation")]
    SessionStopped,
}

#[derive(Debug, Error)]
pub enum CommandSessionError {
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
