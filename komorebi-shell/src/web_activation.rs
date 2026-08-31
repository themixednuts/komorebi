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

use crate::WebSearchEndpoint;
use crate::WebSearchRequest;
use crate::WebSearchTarget;

/// Terminal result of a user-initiated URI launch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WebLaunchDisposition {
    Launched,
    Rejected,
}

/// Platform-neutral failure returned by a URI-launch adapter.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
#[error("web URI activation failed: {message}")]
pub struct WebLaunchFailure {
    native_code: Option<i32>,
    message: Box<str>,
}

impl WebLaunchFailure {
    /// Creates a failure without exposing platform-specific error types.
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

/// Consumer-owned capability for launching one already-authorized HTTPS URI.
pub trait WebUriLauncher: Send + Sync + 'static {
    fn launch(
        &self,
        target: WebSearchTarget,
    ) -> impl Future<Output = Result<WebLaunchDisposition, WebLaunchFailure>> + Send;
}

/// Maximum number of launch requests admitted but not yet completed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WebActivationQueueCapacity(NonZeroUsize);

impl WebActivationQueueCapacity {
    /// Creates a bounded capacity when one shutdown slot can also be reserved.
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

/// Owned web-activation actor and its shutdown join handle.
pub struct WebActivationService {
    client: WebActivationClient,
    worker: Option<JoinHandle<()>>,
}

impl WebActivationService {
    /// Starts one sequential owner for the configured endpoint and native adapter.
    #[must_use]
    pub fn start(
        endpoint: WebSearchEndpoint,
        launcher: impl WebUriLauncher,
        capacity: WebActivationQueueCapacity,
    ) -> Self {
        let (commands, receiver) = mpsc::channel(capacity.channel_slots());
        let admission = Arc::new(Semaphore::new(capacity.request_slots()));
        let worker = tokio::spawn(run_worker(receiver, endpoint, launcher));
        Self {
            client: WebActivationClient {
                commands,
                admission,
            },
            worker: Some(worker),
        }
    }

    #[must_use]
    pub fn client(&self) -> WebActivationClient {
        self.client.clone()
    }

    /// Stops admission, drains every admitted launch, and joins the owner task.
    ///
    /// Cancelling this future still leaves the worker owning all admitted native
    /// side effects; [`Drop`] preserves the shutdown request.
    ///
    /// # Errors
    ///
    /// Returns when the actor task panics or is cancelled by its runtime.
    pub async fn shutdown(mut self) -> Result<(), WebActivationShutdownError> {
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
            .try_send(WebActivationCommand::Shutdown);
    }
}

impl Drop for WebActivationService {
    fn drop(&mut self) {
        self.request_shutdown();
    }
}

/// Cloneable admission handle for the owned web-activation actor.
#[derive(Clone)]
pub struct WebActivationClient {
    commands: mpsc::Sender<WebActivationCommand>,
    admission: Arc<Semaphore>,
}

/// Runtime state of the user-configurable web-search broker.
#[derive(Clone)]
pub enum WebSearchBroker {
    Configured(WebActivationClient),
    Unconfigured,
}

impl WebActivationClient {
    /// Admits one user-initiated request and returns its independently droppable
    /// completion interest.
    ///
    /// Once this method returns a ticket, the actor owns the native side effect.
    /// Dropping the ticket only abandons the caller's interest in its result.
    ///
    /// # Errors
    ///
    /// Returns [`WebActivationSubmitError::Stopped`] after shutdown begins or
    /// if the owner task has terminated.
    pub async fn submit(
        &self,
        request: WebSearchRequest,
    ) -> Result<WebActivationTicket, WebActivationSubmitError> {
        let permit = Arc::clone(&self.admission)
            .acquire_owned()
            .await
            .map_err(|_| WebActivationSubmitError::Stopped)?;
        let (result, completion) = oneshot::channel();
        self.commands
            .send(WebActivationCommand::Launch {
                request,
                result,
                _permit: permit,
            })
            .await
            .map_err(|_| WebActivationSubmitError::Stopped)?;
        Ok(WebActivationTicket { completion })
    }
}

/// Completion interest for one admitted launch.
#[must_use = "dropping a ticket abandons only result observation, not the admitted launch"]
pub struct WebActivationTicket {
    completion: oneshot::Receiver<Result<WebLaunchDisposition, WebLaunchFailure>>,
}

impl WebActivationTicket {
    /// Waits for the broker-owned native attempt.
    ///
    /// # Errors
    ///
    /// Returns when the actor terminates before publishing a result.
    pub async fn complete(
        self,
    ) -> Result<Result<WebLaunchDisposition, WebLaunchFailure>, WebActivationCompletionError> {
        self.completion
            .await
            .map_err(|_| WebActivationCompletionError::Stopped)
    }
}

enum WebActivationCommand {
    Launch {
        request: WebSearchRequest,
        result: oneshot::Sender<Result<WebLaunchDisposition, WebLaunchFailure>>,
        _permit: OwnedSemaphorePermit,
    },
    Shutdown,
}

async fn run_worker(
    mut commands: mpsc::Receiver<WebActivationCommand>,
    endpoint: WebSearchEndpoint,
    launcher: impl WebUriLauncher,
) {
    while let Some(command) = commands.recv().await {
        match command {
            WebActivationCommand::Launch {
                request, result, ..
            } => launch(&endpoint, &launcher, request, result).await,
            WebActivationCommand::Shutdown => {
                commands.close();
                while let Some(command) = commands.recv().await {
                    if let WebActivationCommand::Launch {
                        request, result, ..
                    } = command
                    {
                        launch(&endpoint, &launcher, request, result).await;
                    }
                }
                return;
            }
        }
    }
}

async fn launch(
    endpoint: &WebSearchEndpoint,
    launcher: &impl WebUriLauncher,
    request: WebSearchRequest,
    result: oneshot::Sender<Result<WebLaunchDisposition, WebLaunchFailure>>,
) {
    let outcome = launcher.launch(endpoint.target(&request)).await;
    let _ = result.send(outcome);
}

/// Failure to admit a web activation request.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum WebActivationSubmitError {
    #[error("web activation has stopped")]
    Stopped,
}

/// Failure to observe an admitted activation's result.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum WebActivationCompletionError {
    #[error("web activation stopped before publishing the result")]
    Stopped,
}

/// Failure while joining the web-activation owner task.
#[derive(Debug, Error)]
pub enum WebActivationShutdownError {
    #[error("web-activation worker failed: {0}")]
    Worker(#[from] JoinError),
}
