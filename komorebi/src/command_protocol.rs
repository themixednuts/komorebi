use std::path::PathBuf;

use komorebi_command_transport::CommandProtocolServer;
use komorebi_command_transport::EstablishedSession;
use komorebi_command_transport::PendingProtocolSession;
use komorebi_command_transport::SessionAcceptance;
use komorebi_command_transport::SessionReply;
use komorebi_command_transport::SessionRequest;
use komorebi_command_transport::TransportError;
use komorebi_protocol::AuthoritySummary;
use komorebi_protocol::ManagerEpoch;
use komorebi_protocol::ServerSupport;
use thiserror::Error;
use tokio::sync::watch;
use tokio::task::JoinError;
use tokio::task::JoinHandle;
use tokio::task::JoinSet;

use crate::command_control::CommandControlError;
use crate::command_control::CommandControlPlane;
use crate::command_control::CommandControlPlaneOwner;

pub struct CommandProtocol {
    shutdown: watch::Sender<bool>,
    server: JoinHandle<Result<(), CommandProtocolError>>,
    control_owner: CommandControlPlaneOwner,
}

impl CommandProtocol {
    /// Binds the authenticated named-pipe endpoint and starts its durable
    /// control plane under the process Tokio runtime.
    ///
    /// # Errors
    ///
    /// Returns [`CommandProtocolError`] when the durable store cannot open or
    /// the endpoint cannot bind.
    pub async fn start(
        manager_epoch: ManagerEpoch,
        ledger_path: PathBuf,
    ) -> Result<Self, CommandProtocolError> {
        let server = CommandProtocolServer::bind_current(
            manager_epoch,
            ServerSupport::v1(),
            AuthoritySummary::command_owner(),
        )?;
        tracing::info!(endpoint = ?server.endpoint(), "bound authenticated command protocol");
        let (control, control_owner) = CommandControlPlane::start(ledger_path).await?;

        let (shutdown, shutdown_rx) = watch::channel(false);
        let server = tokio::spawn(run_server(server, control, shutdown_rx));
        Ok(Self {
            shutdown,
            server,
            control_owner,
        })
    }

    /// Waits for Ctrl-C or a server failure, then stops accepting, cancels and
    /// drains session tasks, and joins the SQLite owner.
    ///
    /// # Errors
    ///
    /// Returns [`CommandProtocolError`] when an owned task or the control plane
    /// fails during shutdown.
    pub async fn run_until_ctrl_c(mut self) -> Result<(), CommandProtocolError> {
        enum StopTrigger {
            CtrlC(Result<(), std::io::Error>),
            Server(Result<Result<(), CommandProtocolError>, JoinError>),
        }

        let trigger = tokio::select! {
            signal = tokio::signal::ctrl_c() => StopTrigger::CtrlC(signal),
            server = &mut self.server => StopTrigger::Server(server),
        };
        let (signal_result, server_result) = match trigger {
            StopTrigger::CtrlC(signal) => {
                let _ = self.shutdown.send(true);
                let server = self.server.await;
                (
                    Some(signal),
                    server
                        .map_err(CommandProtocolError::from)
                        .and_then(std::convert::identity),
                )
            }
            StopTrigger::Server(server) => (
                None,
                server
                    .map_err(CommandProtocolError::from)
                    .and_then(|result| match result {
                        Ok(()) => Err(CommandProtocolError::ServerStopped),
                        Err(error) => Err(error),
                    }),
            ),
        };
        let control_result = self.control_owner.shutdown().await;
        if let Some(signal) = signal_result {
            signal?;
        }
        server_result?;
        control_result?;
        Ok(())
    }
}

async fn run_server(
    mut server: CommandProtocolServer,
    control: CommandControlPlane,
    mut shutdown: watch::Receiver<bool>,
) -> Result<(), CommandProtocolError> {
    let mut sessions = JoinSet::new();
    loop {
        tokio::select! {
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    break;
                }
            }
            accepted = server.accept() => {
                let pending = accepted?;
                sessions.spawn(run_pending_session(pending, control.clone()));
            }
            completed = sessions.join_next(), if !sessions.is_empty() => {
                report_session_completion(completed);
            }
        }
    }

    sessions.abort_all();
    while let Some(completed) = sessions.join_next().await {
        report_session_completion(Some(completed));
    }
    Ok(())
}

fn report_session_completion(
    completed: Option<Result<Result<(), CommandProtocolError>, JoinError>>,
) {
    match completed {
        Some(Ok(Ok(()))) => {}
        Some(Ok(Err(CommandProtocolError::Transport(error)))) => {
            tracing::debug!(%error, "command protocol session closed");
        }
        Some(Ok(Err(error))) => tracing::error!(%error, "command protocol session failed"),
        Some(Err(error)) if error.is_cancelled() => {}
        Some(Err(error)) => tracing::error!(%error, "command protocol session task failed"),
        None => {}
    }
}

async fn run_pending_session(
    pending: PendingProtocolSession,
    control: CommandControlPlane,
) -> Result<(), CommandProtocolError> {
    match pending.negotiate().await? {
        SessionAcceptance::Established(session) => run_session(*session, control).await,
        SessionAcceptance::Unsupported { peer } => {
            tracing::debug!(principal = ?peer.principal_id(), "rejected unsupported command protocol session");
            Ok(())
        }
    }
}

async fn run_session(
    mut session: EstablishedSession,
    control: CommandControlPlane,
) -> Result<(), CommandProtocolError> {
    loop {
        let authenticated = session.receive_request().await?;
        let reply_target = authenticated.reply_target();
        let principal = authenticated.authority().principal();
        let reply = match authenticated.into_request() {
            SessionRequest::LeaseInvocationIds(request) => {
                SessionReply::InvocationLease(control.lease(principal, request).await?)
            }
            SessionRequest::InvocationStatus(request) => {
                SessionReply::InvocationStatus(control.status(principal, request).await?)
            }
            SessionRequest::CancelInvocation(request) => {
                SessionReply::CancelInvocation(control.cancel(principal, request).await?)
            }
            SessionRequest::GetCatalog(_) => {
                return Err(CommandProtocolError::CatalogNotConnected);
            }
            SessionRequest::Invoke(_) => return Err(CommandProtocolError::InvokeNotConnected),
        };
        session.send_reply(reply_target, reply).await?;
    }
}

#[derive(Debug, Error)]
pub enum CommandProtocolError {
    #[error("command transport failed: {0}")]
    Transport(#[from] TransportError),
    #[error("command control plane failed: {0}")]
    Control(#[from] CommandControlError),
    #[error("command protocol owner task failed: {0}")]
    Join(#[from] JoinError),
    #[error("action invocation is not connected to the manager owner yet")]
    InvokeNotConnected,
    #[error("action catalog is not connected to the manager owner yet")]
    CatalogNotConnected,
    #[error("command protocol server stopped before process shutdown")]
    ServerStopped,
    #[error("could not wait for the process shutdown signal: {0}")]
    ShutdownSignal(#[from] std::io::Error),
}
