use std::num::NonZeroUsize;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use thiserror::Error;
use tokio::sync::OwnedSemaphorePermit;
use tokio::sync::Semaphore;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::task::JoinError;
use tokio::task::JoinHandle;

use crate::LpacSessionError;
use crate::LpacWorkerLauncher;
use crate::PluginLimits;
use crate::PluginLoadFailure;
use crate::PluginLoadReport;
use crate::PluginManifest;
use crate::PluginProgram;
use crate::SandboxIdentity;

/// Maximum number of admitted reloads that may be queued or executing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PluginHostQueueCapacity(NonZeroUsize);

impl PluginHostQueueCapacity {
    #[must_use]
    pub const fn new(capacity: usize) -> Option<Self> {
        if capacity == usize::MAX {
            return None;
        }
        match NonZeroUsize::new(capacity) {
            Some(capacity) => Some(Self(capacity)),
            None => None,
        }
    }

    const fn requests(self) -> usize {
        self.0.get()
    }

    const fn channel(self) -> usize {
        self.requests() + 1
    }
}

/// Owned lifecycle for one plugin's isolated worker and last-good VM.
pub struct PluginHostService {
    client: PluginHostClient,
    owner: Option<JoinHandle<Result<(), LpacSessionError>>>,
    initial_load: PluginLoadReport,
}

impl PluginHostService {
    /// Starts an LPAC worker and publishes the service only after the first VM loads.
    pub async fn start(
        worker: PathBuf,
        manifest: PluginManifest,
        limits: PluginLimits,
        program: PluginProgram,
        capacity: PluginHostQueueCapacity,
    ) -> Result<Self, PluginHostStartError> {
        let (sender, receiver) = mpsc::channel(capacity.channel());
        let permits = Arc::new(Semaphore::new(capacity.requests()));
        let (ready_sender, ready_receiver) = oneshot::channel();
        let owner = tokio::task::spawn_blocking(move || {
            run_owner(&worker, manifest, limits, program, receiver, ready_sender)
        });

        match ready_receiver.await {
            Ok(Ok(initial_load)) => Ok(Self {
                client: PluginHostClient { sender, permits },
                owner: Some(owner),
                initial_load,
            }),
            Ok(Err(failure)) => {
                join_startup_owner(owner).await?;
                Err(PluginHostStartError::Rejected(failure))
            }
            Err(_) => match owner.await {
                Ok(Ok(())) => Err(PluginHostStartError::StoppedBeforeReady),
                Ok(Err(source)) => Err(PluginHostStartError::Session(source)),
                Err(source) => Err(PluginHostStartError::Owner(source)),
            },
        }
    }

    #[must_use]
    pub const fn initial_load(&self) -> &PluginLoadReport {
        &self.initial_load
    }

    #[must_use]
    pub fn client(&self) -> PluginHostClient {
        self.client.clone()
    }

    /// Stops admission, asks the worker to exit, and joins its blocking owner.
    pub async fn shutdown(mut self) -> Result<(), PluginHostShutdownError> {
        self.client.permits.close();
        let _ = self.client.sender.send(OwnerCommand::Shutdown).await;
        let Some(owner) = self.owner.take() else {
            return Ok(());
        };
        match owner.await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(source)) => Err(PluginHostShutdownError::Session(source)),
            Err(source) => Err(PluginHostShutdownError::Owner(source)),
        }
    }
}

impl Drop for PluginHostService {
    fn drop(&mut self) {
        self.client.permits.close();
        let _ = self.client.sender.try_send(OwnerCommand::Shutdown);
    }
}

/// Cloneable, bounded reload port that does not own worker shutdown.
#[derive(Clone)]
pub struct PluginHostClient {
    sender: mpsc::Sender<OwnerCommand>,
    permits: Arc<Semaphore>,
}

impl PluginHostClient {
    /// Transactionally replaces the active VM after the new program loads successfully.
    pub async fn reload(
        &self,
        program: PluginProgram,
    ) -> Result<PluginLoadReport, PluginReloadError> {
        let permit = Arc::clone(&self.permits)
            .acquire_owned()
            .await
            .map_err(|_| PluginReloadError::Stopped)?;
        let (reply, result) = oneshot::channel();
        self.sender
            .send(OwnerCommand::Reload {
                program,
                reply,
                _permit: permit,
            })
            .await
            .map_err(|_| PluginReloadError::Stopped)?;
        result.await.map_err(|_| PluginReloadError::Stopped)?
    }
}

enum OwnerCommand {
    Reload {
        program: PluginProgram,
        reply: oneshot::Sender<Result<PluginLoadReport, PluginReloadError>>,
        _permit: OwnedSemaphorePermit,
    },
    Shutdown,
}

fn run_owner(
    worker: &Path,
    manifest: PluginManifest,
    limits: PluginLimits,
    program: PluginProgram,
    mut commands: mpsc::Receiver<OwnerCommand>,
    ready: oneshot::Sender<Result<PluginLoadReport, PluginLoadFailure>>,
) -> Result<(), LpacSessionError> {
    let plugin = manifest.id().clone();
    let launcher = LpacWorkerLauncher::new(SandboxIdentity::for_plugin(&plugin));
    let mut session = match launcher.launch_session(worker, plugin) {
        Ok(session) => session,
        Err(error) => return Err(LpacSessionError::Launch(error)),
    };
    session.await_ready()?;
    let initial = match session.initialize(manifest, limits, program) {
        Ok(report) => report,
        Err(LpacSessionError::Rejected(failure)) => {
            let _ = ready.send(Err(failure));
            return session.shutdown();
        }
        Err(error) => return Err(error),
    };
    if ready.send(Ok(initial)).is_err() {
        return session.shutdown();
    }

    while let Some(command) = commands.blocking_recv() {
        match command {
            OwnerCommand::Reload { program, reply, .. } => match session.reload(program) {
                Ok(report) => {
                    let _ = reply.send(Ok(report));
                }
                Err(LpacSessionError::Rejected(failure)) => {
                    let _ = reply.send(Err(PluginReloadError::Rejected(failure)));
                }
                Err(error) => return Err(error),
            },
            OwnerCommand::Shutdown => return session.shutdown(),
        }
    }
    session.shutdown()
}

async fn join_startup_owner(
    owner: JoinHandle<Result<(), LpacSessionError>>,
) -> Result<(), PluginHostStartError> {
    match owner.await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(source)) => Err(PluginHostStartError::Session(source)),
        Err(source) => Err(PluginHostStartError::Owner(source)),
    }
}

#[derive(Debug, Error)]
pub enum PluginHostStartError {
    #[error(transparent)]
    Rejected(PluginLoadFailure),
    #[error("extension owner stopped before publishing readiness")]
    StoppedBeforeReady,
    #[error("extension worker session failed: {0}")]
    Session(LpacSessionError),
    #[error("extension owner task failed: {0}")]
    Owner(JoinError),
}

#[derive(Debug, Error)]
pub enum PluginReloadError {
    #[error(transparent)]
    Rejected(PluginLoadFailure),
    #[error("extension worker has stopped")]
    Stopped,
}

#[derive(Debug, Error)]
pub enum PluginHostShutdownError {
    #[error("extension worker session failed: {0}")]
    Session(LpacSessionError),
    #[error("extension owner task failed: {0}")]
    Owner(JoinError),
}
