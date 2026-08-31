use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::Arc;

use thiserror::Error;
use tokio::sync::OwnedSemaphorePermit;
use tokio::sync::Semaphore;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::task::JoinError;
use tokio::task::JoinHandle;

use crate::FileIndex;
use crate::FileIndexBuildError;
use crate::FileSearchLimit;
use crate::FileSearchMatch;
use crate::OpaquePathId;

/// Maximum concurrently queued or executing file-search requests.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FileSearchQueueCapacity(NonZeroUsize);

impl FileSearchQueueCapacity {
    /// Creates a nonzero capacity while reserving address space for the
    /// service's out-of-band shutdown message.
    pub const fn new(capacity: usize) -> Option<Self> {
        if capacity == usize::MAX {
            return None;
        }
        match NonZeroUsize::new(capacity) {
            Some(capacity) => Some(Self(capacity)),
            None => None,
        }
    }

    const fn request_capacity(self) -> usize {
        self.0.get()
    }

    const fn channel_capacity(self) -> usize {
        self.request_capacity() + 1
    }
}

/// The owned lifecycle for one immutable file index and blocking worker.
pub struct FileSearchService {
    client: FileSearchClient,
    worker: Option<JoinHandle<()>>,
}

impl FileSearchService {
    /// Builds the index on Tokio's blocking pool and starts its single owner.
    ///
    /// # Errors
    ///
    /// Returns an error when indexing fails or the worker exits before it can
    /// publish readiness.
    pub async fn start(
        root: PathBuf,
        capacity: FileSearchQueueCapacity,
    ) -> Result<Self, FileSearchStartError> {
        let (sender, receiver) = mpsc::channel(capacity.channel_capacity());
        let permits = Arc::new(Semaphore::new(capacity.request_capacity()));
        let (ready_sender, ready_receiver) = oneshot::channel();
        let worker = tokio::task::spawn_blocking(move || {
            run_worker(root, receiver, ready_sender);
        });

        match ready_receiver.await {
            Ok(Ok(())) => Ok(Self {
                client: FileSearchClient { sender, permits },
                worker: Some(worker),
            }),
            Ok(Err(source)) => match worker.await {
                Ok(()) => Err(FileSearchStartError::Build(source)),
                Err(source) => Err(FileSearchStartError::Worker(source)),
            },
            Err(_) => match worker.await {
                Ok(()) => Err(FileSearchStartError::StoppedBeforeReady),
                Err(source) => Err(FileSearchStartError::Worker(source)),
            },
        }
    }

    /// Returns a cloneable request client that does not own worker shutdown.
    pub fn client(&self) -> FileSearchClient {
        self.client.clone()
    }

    /// Stops request admission and joins the blocking worker.
    ///
    /// # Errors
    ///
    /// Returns an error only when the worker panicked or was cancelled.
    pub async fn shutdown(mut self) -> Result<(), FileSearchShutdownError> {
        self.client.permits.close();
        let _ = self.client.sender.send(Command::Shutdown).await;
        let Some(worker) = self.worker.take() else {
            return Ok(());
        };
        worker.await.map_err(FileSearchShutdownError)
    }
}

impl Drop for FileSearchService {
    fn drop(&mut self) {
        self.client.permits.close();
        let _ = self.client.sender.try_send(Command::Shutdown);
    }
}

/// Cloneable async access to a file index owned by [`FileSearchService`].
#[derive(Clone)]
pub struct FileSearchClient {
    sender: mpsc::Sender<Command>,
    permits: Arc<Semaphore>,
}

impl FileSearchClient {
    /// Searches the owned immutable index.
    ///
    /// Cancelling this future drops only the caller's result interest. The
    /// worker may finish an admitted search and remains valid for later calls.
    ///
    /// # Errors
    ///
    /// Returns [`FileSearchRequestError::Stopped`] after service shutdown or
    /// when the worker exits.
    pub async fn search(
        &self,
        query: impl Into<String>,
        limit: FileSearchLimit,
    ) -> Result<Vec<FileSearchMatch>, FileSearchRequestError> {
        let query = query.into();
        let permit = self.acquire_permit().await?;
        let (reply, result) = oneshot::channel();
        self.sender
            .send(Command::Search {
                query,
                limit,
                reply,
                permit,
            })
            .await
            .map_err(|_| FileSearchRequestError::Stopped)?;
        result.await.map_err(|_| FileSearchRequestError::Stopped)
    }

    /// Resolves an opaque identity inside the worker that owns its index.
    ///
    /// # Errors
    ///
    /// Returns [`FileSearchRequestError::Stopped`] after service shutdown or
    /// when the worker exits.
    pub async fn resolve(
        &self,
        id: OpaquePathId,
    ) -> Result<Option<PathBuf>, FileSearchRequestError> {
        let permit = self.acquire_permit().await?;
        let (reply, result) = oneshot::channel();
        self.sender
            .send(Command::Resolve { id, reply, permit })
            .await
            .map_err(|_| FileSearchRequestError::Stopped)?;
        result.await.map_err(|_| FileSearchRequestError::Stopped)
    }

    async fn acquire_permit(&self) -> Result<OwnedSemaphorePermit, FileSearchRequestError> {
        Arc::clone(&self.permits)
            .acquire_owned()
            .await
            .map_err(|_| FileSearchRequestError::Stopped)
    }
}

enum Command {
    Search {
        query: String,
        limit: FileSearchLimit,
        reply: oneshot::Sender<Vec<FileSearchMatch>>,
        permit: OwnedSemaphorePermit,
    },
    Resolve {
        id: OpaquePathId,
        reply: oneshot::Sender<Option<PathBuf>>,
        permit: OwnedSemaphorePermit,
    },
    Shutdown,
}

fn run_worker(
    root: PathBuf,
    mut receiver: mpsc::Receiver<Command>,
    ready: oneshot::Sender<Result<(), FileIndexBuildError>>,
) {
    let index = match FileIndex::build(root) {
        Ok(index) => index,
        Err(source) => {
            let _ = ready.send(Err(source));
            return;
        }
    };
    if ready.send(Ok(())).is_err() {
        return;
    }

    while let Some(command) = receiver.blocking_recv() {
        match command {
            Command::Search {
                query,
                limit,
                reply,
                permit,
            } => {
                let _ = reply.send(index.search(&query, limit));
                drop(permit);
            }
            Command::Resolve { id, reply, permit } => {
                let resolved = index.resolve(&id).map(PathBuf::from);
                let _ = reply.send(resolved);
                drop(permit);
            }
            Command::Shutdown => break,
        }
    }
}

/// Failure to start the owned file-search worker.
#[derive(Debug, Error)]
pub enum FileSearchStartError {
    /// The index root could not be scanned.
    #[error(transparent)]
    Build(#[from] FileIndexBuildError),
    /// The worker task failed.
    #[error("file-search worker failed: {0}")]
    Worker(#[source] JoinError),
    /// The worker exited cleanly without publishing readiness.
    #[error("file-search worker stopped before publishing readiness")]
    StoppedBeforeReady,
}

/// Failure to submit a request or receive its result.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum FileSearchRequestError {
    /// The service no longer accepts or completes requests.
    #[error("file-search service is stopped")]
    Stopped,
}

/// Failure while joining the owned file-search worker.
#[derive(Debug, Error)]
#[error("file-search worker failed during shutdown: {0}")]
pub struct FileSearchShutdownError(#[source] JoinError);
