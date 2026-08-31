use std::future::Future;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::Arc;

use komorebi_search::FileSearchClient;
use komorebi_search::FileSearchRequestError;
use komorebi_search::OpaquePathId;
use thiserror::Error;
use tokio::sync::OwnedSemaphorePermit;
use tokio::sync::Semaphore;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::task::JoinError;
use tokio::task::JoinHandle;

/// Consumer-owned capability for launching one already-resolved exact path.
pub trait FileLauncher: Send + Sync + 'static {
    /// Requests native activation of `path`.
    ///
    /// # Errors
    ///
    /// Returns a platform-neutral failure when the native shell rejects the path.
    fn launch(&self, path: PathBuf) -> impl Future<Output = Result<(), FileLaunchFailure>> + Send;
}

/// Platform-neutral failure returned by a native file-launch adapter.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
#[error("file activation failed: {message}")]
pub struct FileLaunchFailure {
    native_code: Option<i32>,
    message: Box<str>,
}

impl FileLaunchFailure {
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

/// Maximum number of file activations admitted but not yet completed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FileActivationQueueCapacity(NonZeroUsize);

impl FileActivationQueueCapacity {
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

/// Single owner of opaque-path resolution followed by native activation.
pub struct FileActivationService {
    client: FileActivationClient,
    worker: Option<JoinHandle<()>>,
}

impl FileActivationService {
    #[must_use]
    pub fn start(
        files: FileSearchClient,
        launcher: impl FileLauncher,
        capacity: FileActivationQueueCapacity,
    ) -> Self {
        let (commands, receiver) = mpsc::channel(capacity.channel_slots());
        let admission = Arc::new(Semaphore::new(capacity.request_slots()));
        let worker = tokio::spawn(run_worker(receiver, files, launcher));
        Self {
            client: FileActivationClient {
                commands,
                admission,
            },
            worker: Some(worker),
        }
    }

    #[must_use]
    pub fn client(&self) -> FileActivationClient {
        self.client.clone()
    }

    /// Stops admission, drains accepted activations, and joins the owner task.
    ///
    /// # Errors
    ///
    /// Returns when the owner task panics or is cancelled by its runtime.
    pub async fn shutdown(mut self) -> Result<(), FileActivationShutdownError> {
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
            .try_send(FileActivationCommand::Shutdown);
    }
}

impl Drop for FileActivationService {
    fn drop(&mut self) {
        self.request_shutdown();
    }
}

/// Cloneable admission handle for the owned file-activation actor.
#[derive(Clone)]
pub struct FileActivationClient {
    commands: mpsc::Sender<FileActivationCommand>,
    admission: Arc<Semaphore>,
}

impl FileActivationClient {
    /// Admits an opaque identity and returns independently droppable result interest.
    ///
    /// # Errors
    ///
    /// Returns [`FileActivationSubmitError::Stopped`] after shutdown begins.
    pub async fn submit(
        &self,
        id: OpaquePathId,
    ) -> Result<FileActivationTicket, FileActivationSubmitError> {
        let permit = Arc::clone(&self.admission)
            .acquire_owned()
            .await
            .map_err(|_| FileActivationSubmitError::Stopped)?;
        let (result, completion) = oneshot::channel();
        self.commands
            .send(FileActivationCommand::Launch {
                id,
                result,
                _permit: permit,
            })
            .await
            .map_err(|_| FileActivationSubmitError::Stopped)?;
        Ok(FileActivationTicket { completion })
    }
}

/// Completion interest for one admitted file activation.
#[must_use = "dropping the ticket abandons only observation, not the admitted activation"]
pub struct FileActivationTicket {
    completion: oneshot::Receiver<Result<(), FileActivationFailure>>,
}

impl FileActivationTicket {
    /// Waits for the actor-owned resolution and launch.
    ///
    /// # Errors
    ///
    /// Returns when the actor stops before publishing the terminal result.
    pub async fn complete(
        self,
    ) -> Result<Result<(), FileActivationFailure>, FileActivationCompletionError> {
        self.completion
            .await
            .map_err(|_| FileActivationCompletionError::Stopped)
    }
}

enum FileActivationCommand {
    Launch {
        id: OpaquePathId,
        result: oneshot::Sender<Result<(), FileActivationFailure>>,
        _permit: OwnedSemaphorePermit,
    },
    Shutdown,
}

async fn run_worker(
    mut commands: mpsc::Receiver<FileActivationCommand>,
    files: FileSearchClient,
    launcher: impl FileLauncher,
) {
    while let Some(command) = commands.recv().await {
        match command {
            FileActivationCommand::Launch { id, result, .. } => {
                launch(&files, &launcher, id, result).await;
            }
            FileActivationCommand::Shutdown => {
                commands.close();
                while let Some(command) = commands.recv().await {
                    if let FileActivationCommand::Launch { id, result, .. } = command {
                        launch(&files, &launcher, id, result).await;
                    }
                }
                return;
            }
        }
    }
}

async fn launch(
    files: &FileSearchClient,
    launcher: &impl FileLauncher,
    id: OpaquePathId,
    result: oneshot::Sender<Result<(), FileActivationFailure>>,
) {
    let outcome = match files.resolve(id).await {
        Ok(Some(path)) => launcher
            .launch(path)
            .await
            .map_err(FileActivationFailure::Launch),
        Ok(None) => Err(FileActivationFailure::StaleIdentity),
        Err(error) => Err(FileActivationFailure::Resolve(error)),
    };
    let _ = result.send(outcome);
}

/// Terminal failure after file activation was admitted.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum FileActivationFailure {
    #[error("file identity no longer belongs to the active index")]
    StaleIdentity,
    #[error("file identity resolution failed: {0}")]
    Resolve(FileSearchRequestError),
    #[error(transparent)]
    Launch(FileLaunchFailure),
}

/// Failure to admit a file activation.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum FileActivationSubmitError {
    #[error("file activation has stopped")]
    Stopped,
}

/// Failure to observe an admitted activation's result.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum FileActivationCompletionError {
    #[error("file activation stopped before publishing the result")]
    Stopped,
}

/// Failure while joining the file-activation owner task.
#[derive(Debug, Error)]
pub enum FileActivationShutdownError {
    #[error("file-activation worker failed: {0}")]
    Worker(#[from] JoinError),
}
