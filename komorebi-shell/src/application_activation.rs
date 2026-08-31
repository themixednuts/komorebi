use std::future::Future;
use std::num::NonZeroUsize;
use std::sync::Arc;

use thiserror::Error;
use tokio::sync::OwnedSemaphorePermit;
use tokio::sync::Semaphore;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::task::JoinError;
use tokio::task::JoinHandle;

use crate::ApplicationId;

/// Consumer-owned capability for activating an exact Windows Shell application identity.
pub trait ApplicationLauncher: Send + Sync + 'static {
    fn launch(
        &self,
        id: ApplicationId,
    ) -> impl Future<Output = Result<(), ApplicationLaunchFailure>> + Send;
}

/// Platform-neutral failure returned by a native application adapter.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
#[error("application activation failed: {message}")]
pub struct ApplicationLaunchFailure {
    native_code: Option<i32>,
    message: Box<str>,
}

impl ApplicationLaunchFailure {
    #[must_use]
    pub fn new(message: impl Into<Box<str>>) -> Self {
        Self {
            native_code: None,
            message: message.into(),
        }
    }

    pub(crate) fn native(native_code: i32, message: impl Into<Box<str>>) -> Self {
        Self {
            native_code: Some(native_code),
            message: message.into(),
        }
    }

    #[must_use]
    pub const fn native_code(&self) -> Option<i32> {
        self.native_code
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

/// Maximum number of application activations admitted but not yet completed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ApplicationActivationQueueCapacity(NonZeroUsize);

impl ApplicationActivationQueueCapacity {
    #[must_use]
    pub fn new(capacity: usize) -> Option<Self> {
        let capacity = NonZeroUsize::new(capacity)?;
        capacity.get().checked_add(1).map(|_| Self(capacity))
    }

    const fn request_slots(self) -> usize {
        self.0.get()
    }

    fn channel_slots(self) -> usize {
        self.request_slots() + 1
    }
}

/// Single owner of admitted native application launches.
pub struct ApplicationActivationService {
    client: ApplicationActivationClient,
    worker: Option<JoinHandle<()>>,
}

impl ApplicationActivationService {
    #[must_use]
    pub fn start(
        launcher: impl ApplicationLauncher,
        capacity: ApplicationActivationQueueCapacity,
    ) -> Self {
        let (commands, receiver) = mpsc::channel(capacity.channel_slots());
        let admission = Arc::new(Semaphore::new(capacity.request_slots()));
        let worker = tokio::spawn(run_worker(receiver, launcher));
        Self {
            client: ApplicationActivationClient {
                commands,
                admission,
            },
            worker: Some(worker),
        }
    }

    #[must_use]
    pub fn client(&self) -> ApplicationActivationClient {
        self.client.clone()
    }

    pub async fn shutdown(mut self) -> Result<(), ApplicationActivationShutdownError> {
        self.request_shutdown();
        if let Some(worker) = self.worker.take() {
            worker.await?;
        }
        Ok(())
    }

    fn request_shutdown(&self) {
        self.client.admission.close();
        let _ = self
            .client
            .commands
            .try_send(ApplicationActivationCommand::Shutdown);
    }
}

impl Drop for ApplicationActivationService {
    fn drop(&mut self) {
        self.request_shutdown();
    }
}

/// Cloneable admission handle for the owned application-activation actor.
#[derive(Clone)]
pub struct ApplicationActivationClient {
    commands: mpsc::Sender<ApplicationActivationCommand>,
    admission: Arc<Semaphore>,
}

impl ApplicationActivationClient {
    pub async fn submit(
        &self,
        id: ApplicationId,
    ) -> Result<ApplicationActivationTicket, ApplicationActivationSubmitError> {
        let permit = Arc::clone(&self.admission)
            .acquire_owned()
            .await
            .map_err(|_| ApplicationActivationSubmitError::Stopped)?;
        let (result, completion) = oneshot::channel();
        self.commands
            .send(ApplicationActivationCommand::Launch {
                id,
                result,
                _permit: permit,
            })
            .await
            .map_err(|_| ApplicationActivationSubmitError::Stopped)?;
        Ok(ApplicationActivationTicket { completion })
    }
}

/// Completion interest for one admitted application activation.
#[must_use = "dropping the ticket abandons only observation, not the admitted activation"]
pub struct ApplicationActivationTicket {
    completion: oneshot::Receiver<Result<(), ApplicationLaunchFailure>>,
}

impl ApplicationActivationTicket {
    pub async fn complete(
        self,
    ) -> Result<Result<(), ApplicationLaunchFailure>, ApplicationActivationCompletionError> {
        self.completion
            .await
            .map_err(|_| ApplicationActivationCompletionError::Stopped)
    }
}

enum ApplicationActivationCommand {
    Launch {
        id: ApplicationId,
        result: oneshot::Sender<Result<(), ApplicationLaunchFailure>>,
        _permit: OwnedSemaphorePermit,
    },
    Shutdown,
}

async fn run_worker(
    mut commands: mpsc::Receiver<ApplicationActivationCommand>,
    launcher: impl ApplicationLauncher,
) {
    while let Some(command) = commands.recv().await {
        match command {
            ApplicationActivationCommand::Launch { id, result, .. } => {
                let _ = result.send(launcher.launch(id).await);
            }
            ApplicationActivationCommand::Shutdown => {
                commands.close();
                while let Some(command) = commands.recv().await {
                    if let ApplicationActivationCommand::Launch { id, result, .. } = command {
                        let _ = result.send(launcher.launch(id).await);
                    }
                }
                return;
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum ApplicationActivationSubmitError {
    #[error("application activation has stopped")]
    Stopped,
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum ApplicationActivationCompletionError {
    #[error("application activation stopped before publishing the result")]
    Stopped,
}

#[derive(Debug, Error)]
pub enum ApplicationActivationShutdownError {
    #[error("application-activation worker failed: {0}")]
    Worker(#[from] JoinError),
}
