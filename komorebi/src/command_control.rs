use std::path::PathBuf;
use std::thread::JoinHandle;

use komorebi_command_store::DurableInvocationLedger;
use komorebi_command_store::LeaseDecision;
use komorebi_command_store::LeaseRequest;
use komorebi_command_store::LedgerError;
use komorebi_command_store::LedgerTimestamp;
use komorebi_command_store::NewLeaseDecision;
use komorebi_command_store::TimeError;
use komorebi_protocol::CancelInvocationReply;
use komorebi_protocol::CancelInvocationRequest;
use komorebi_protocol::InvocationIdentityError;
use komorebi_protocol::InvocationLeaseRejection;
use komorebi_protocol::InvocationLeaseReply;
use komorebi_protocol::InvocationLeaseRequest;
use komorebi_protocol::InvocationNamespaceId;
use komorebi_protocol::InvocationStatusReply;
use komorebi_protocol::InvocationStatusRequest;
use komorebi_protocol::LaneLimits;
use komorebi_protocol::PrincipalId;
use thiserror::Error;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use uuid::Uuid;

#[derive(Clone)]
pub struct CommandControlPlane {
    requests: mpsc::Sender<ControlRequest>,
}

pub struct CommandControlPlaneOwner {
    requests: mpsc::Sender<ControlRequest>,
    stopped: oneshot::Receiver<()>,
    worker: JoinHandle<()>,
}

enum ControlRequest {
    Lease {
        principal: PrincipalId,
        request: InvocationLeaseRequest,
        reply: oneshot::Sender<Result<InvocationLeaseReply, CommandControlError>>,
    },
    Status {
        principal: PrincipalId,
        request: InvocationStatusRequest,
        reply: oneshot::Sender<Result<InvocationStatusReply, CommandControlError>>,
    },
    Cancel {
        principal: PrincipalId,
        request: CancelInvocationRequest,
        reply: oneshot::Sender<Result<CancelInvocationReply, CommandControlError>>,
    },
    Shutdown(oneshot::Sender<()>),
}

impl CommandControlPlane {
    /// Starts the one thread that owns the typed SQLite ledger.
    ///
    /// The runtime never blocks on SQLite. Cancellation before a bounded send
    /// leaves the request with the caller; cancellation after admission only
    /// abandons the reply while the ledger owner completes the transaction.
    ///
    /// # Errors
    ///
    /// Returns [`CommandControlError`] when the worker cannot start or the
    /// durable ledger cannot be opened.
    pub async fn start(
        path: PathBuf,
    ) -> Result<(Self, CommandControlPlaneOwner), CommandControlError> {
        let capacity = usize::try_from(LaneLimits::CONTROL.max_frames())
            .map_err(|_| CommandControlError::CapacityOutsideAddressSpace)?;
        let (request_tx, mut request_rx) = mpsc::channel(capacity);
        let (started_tx, started_rx) = oneshot::channel();
        let (stopped_tx, stopped_rx) = oneshot::channel();
        let worker = std::thread::Builder::new()
            .name("command-control-ledger".to_owned())
            .spawn(move || {
                match DurableInvocationLedger::open(&path) {
                    Ok(mut ledger) => {
                        let _ = started_tx.send(Ok(()));
                        run_ledger(&mut ledger, &mut request_rx);
                    }
                    Err(error) => {
                        let _ = started_tx.send(Err(error));
                    }
                }
                let _ = stopped_tx.send(());
            })?;

        match started_rx.await {
            Ok(Ok(())) => {
                let control = Self {
                    requests: request_tx.clone(),
                };
                let owner = CommandControlPlaneOwner {
                    requests: request_tx,
                    stopped: stopped_rx,
                    worker,
                };
                Ok((control, owner))
            }
            Ok(Err(error)) => Err(error.into()),
            Err(_) => Err(CommandControlError::WorkerStopped),
        }
    }

    pub async fn lease(
        &self,
        principal: PrincipalId,
        request: InvocationLeaseRequest,
    ) -> Result<InvocationLeaseReply, CommandControlError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.requests
            .send(ControlRequest::Lease {
                principal,
                request,
                reply: reply_tx,
            })
            .await
            .map_err(|_| CommandControlError::WorkerStopped)?;
        reply_rx
            .await
            .map_err(|_| CommandControlError::WorkerStopped)?
    }

    pub async fn status(
        &self,
        principal: PrincipalId,
        request: InvocationStatusRequest,
    ) -> Result<InvocationStatusReply, CommandControlError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.requests
            .send(ControlRequest::Status {
                principal,
                request,
                reply: reply_tx,
            })
            .await
            .map_err(|_| CommandControlError::WorkerStopped)?;
        reply_rx
            .await
            .map_err(|_| CommandControlError::WorkerStopped)?
    }

    pub async fn cancel(
        &self,
        principal: PrincipalId,
        request: CancelInvocationRequest,
    ) -> Result<CancelInvocationReply, CommandControlError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.requests
            .send(ControlRequest::Cancel {
                principal,
                request,
                reply: reply_tx,
            })
            .await
            .map_err(|_| CommandControlError::WorkerStopped)?;
        reply_rx
            .await
            .map_err(|_| CommandControlError::WorkerStopped)?
    }
}

impl CommandControlPlaneOwner {
    /// Drains admitted operations and joins the ledger owner.
    ///
    /// # Errors
    ///
    /// Returns [`CommandControlError`] if the owner has already stopped or its
    /// thread panicked.
    pub async fn shutdown(self) -> Result<(), CommandControlError> {
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        self.requests
            .send(ControlRequest::Shutdown(shutdown_tx))
            .await
            .map_err(|_| CommandControlError::WorkerStopped)?;
        shutdown_rx
            .await
            .map_err(|_| CommandControlError::WorkerStopped)?;
        self.stopped
            .await
            .map_err(|_| CommandControlError::WorkerStopped)?;
        self.worker
            .join()
            .map_err(|_| CommandControlError::WorkerPanicked)
    }
}

fn run_ledger(ledger: &mut DurableInvocationLedger, requests: &mut mpsc::Receiver<ControlRequest>) {
    while let Some(request) = requests.blocking_recv() {
        match request {
            ControlRequest::Lease {
                principal,
                request,
                reply,
            } => {
                let _ = reply.send(lease(ledger, principal, request));
            }
            ControlRequest::Status {
                principal,
                request,
                reply,
            } => {
                let result = ledger
                    .status(principal, request.invocation_id())
                    .map(komorebi_command_store::StatusDecision::into_reply)
                    .map_err(Into::into);
                let _ = reply.send(result);
            }
            ControlRequest::Cancel {
                principal,
                request,
                reply,
            } => {
                let result = LedgerTimestamp::now()
                    .map_err(CommandControlError::from)
                    .and_then(|timestamp| {
                        ledger
                            .cancel_invocation(principal, request.invocation_id(), timestamp)
                            .map_err(Into::into)
                    });
                let _ = reply.send(result);
            }
            ControlRequest::Shutdown(reply) => {
                let _ = reply.send(());
                break;
            }
        }
    }
}

fn lease(
    ledger: &mut DurableInvocationLedger,
    principal: PrincipalId,
    request: InvocationLeaseRequest,
) -> Result<InvocationLeaseReply, CommandControlError> {
    let Some(namespace) = request.namespace() else {
        return lease_new(ledger, principal, request.count());
    };
    let decision = ledger.lease(LeaseRequest {
        namespace,
        principal,
        count: request.count(),
    })?;
    Ok(match decision {
        LeaseDecision::Issued(lease) => InvocationLeaseReply::Issued(lease),
        LeaseDecision::UnknownNamespace | LeaseDecision::PrincipalConflict => {
            InvocationLeaseReply::Rejected(InvocationLeaseRejection::UnknownNamespace)
        }
        LeaseDecision::CapacityFull => {
            InvocationLeaseReply::Rejected(InvocationLeaseRejection::CapacityFull)
        }
        LeaseDecision::SequenceExhausted => {
            InvocationLeaseReply::Rejected(InvocationLeaseRejection::SequenceExhausted)
        }
    })
}

fn lease_new(
    ledger: &mut DurableInvocationLedger,
    principal: PrincipalId,
    count: std::num::NonZeroU32,
) -> Result<InvocationLeaseReply, CommandControlError> {
    let namespace = InvocationNamespaceId::new(*Uuid::new_v4().as_bytes())?;
    match ledger.lease_new(LeaseRequest {
        namespace,
        principal,
        count,
    })? {
        NewLeaseDecision::Issued(lease) => Ok(InvocationLeaseReply::Issued(lease)),
        NewLeaseDecision::NamespaceCollision => Err(CommandControlError::NamespaceCollision),
    }
}

#[derive(Debug, Error)]
pub enum CommandControlError {
    #[error("durable invocation ledger failed: {0}")]
    Ledger(#[from] LedgerError),
    #[error("invocation identity generation failed: {0}")]
    Identity(#[from] InvocationIdentityError),
    #[error("ledger clock failed: {0}")]
    Time(#[from] TimeError),
    #[error("could not start the command-control ledger thread: {0}")]
    ThreadStart(#[from] std::io::Error),
    #[error("the command-control ledger owner stopped")]
    WorkerStopped,
    #[error("the command-control ledger owner panicked")]
    WorkerPanicked,
    #[error("a generated invocation namespace collided with durable state")]
    NamespaceCollision,
    #[error("the protocol control-lane capacity does not fit this address space")]
    CapacityOutsideAddressSpace,
}

#[cfg(test)]
mod tests {
    use std::error::Error;
    use std::num::NonZeroU32;

    use komorebi_protocol::InvocationId;
    use komorebi_protocol::InvocationSequence;
    use komorebi_protocol::InvocationUnavailable;

    use super::*;

    fn principal(byte: u8) -> Result<PrincipalId, InvocationIdentityError> {
        PrincipalId::new([byte; 32])
    }

    #[tokio::test]
    async fn leases_are_principal_scoped_and_control_queries_are_typed()
    -> Result<(), Box<dyn Error>> {
        let directory = tempfile::tempdir()?;
        let (control, owner) =
            CommandControlPlane::start(directory.path().join("commands.sqlite")).await?;
        let owner_principal = principal(1)?;
        let other_principal = principal(2)?;

        let lease = match control
            .lease(
                owner_principal,
                InvocationLeaseRequest::new(None, NonZeroU32::new(2).ok_or("nonzero count")?),
            )
            .await?
        {
            InvocationLeaseReply::Issued(lease) => lease,
            InvocationLeaseReply::Rejected(reason) => {
                return Err(format!("new namespace was rejected: {reason:?}").into());
            }
        };
        let invocation_id = InvocationId::new(lease.namespace(), InvocationSequence::try_from(1)?);

        assert_eq!(
            control
                .status(owner_principal, InvocationStatusRequest::new(invocation_id))
                .await?,
            InvocationStatusReply::Unavailable(InvocationUnavailable::UnknownInvocation)
        );
        assert_eq!(
            control
                .cancel(owner_principal, CancelInvocationRequest::new(invocation_id))
                .await?,
            CancelInvocationReply::Unavailable(InvocationUnavailable::UnknownInvocation)
        );
        assert_eq!(
            control
                .lease(
                    other_principal,
                    InvocationLeaseRequest::new(Some(lease.namespace()), NonZeroU32::MIN,),
                )
                .await?,
            InvocationLeaseReply::Rejected(InvocationLeaseRejection::UnknownNamespace)
        );

        drop(control);
        owner.shutdown().await?;
        Ok(())
    }
}
