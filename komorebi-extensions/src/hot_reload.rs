mod source;

use std::num::NonZeroUsize;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use notify::Config;
use notify::Event;
use notify::ReadDirectoryChangesWatcher;
use notify::RecursiveMode;
use notify::Watcher;
use parking_lot::Mutex;
use thiserror::Error;
use tokio::runtime::TryCurrentError;
use tokio::sync::Notify;
use tokio::sync::mpsc;
use tokio::task::JoinError;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::PluginHostClient;
use crate::PluginHostQueueCapacity;
use crate::PluginHostService;
use crate::PluginHostShutdownError;
use crate::PluginHostStartError;
use crate::PluginLimits;
use crate::PluginLoadFailure;
use crate::PluginLoadReport;
use crate::PluginManifest;
use crate::PluginReloadError;

pub use source::PluginSourceFile;
pub use source::PluginSourceLoadError;
pub use source::PluginSourceOpenError;

/// Nonzero event-settling duration used only after a native change notification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PluginHotReloadQuietPeriod(Duration);

impl PluginHotReloadQuietPeriod {
    #[must_use]
    pub fn new(period: Duration) -> Option<Self> {
        (!period.is_zero()).then_some(Self(period))
    }

    const fn get(self) -> Duration {
        self.0
    }
}

/// Maximum number of reload outcomes waiting for the consumer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PluginHotReloadEventCapacity(NonZeroUsize);

impl PluginHotReloadEventCapacity {
    #[must_use]
    pub const fn new(capacity: usize) -> Option<Self> {
        match NonZeroUsize::new(capacity) {
            Some(capacity) => Some(Self(capacity)),
            None => None,
        }
    }

    const fn get(self) -> usize {
        self.0.get()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PluginHotReloadSettings {
    host_queue_capacity: PluginHostQueueCapacity,
    event_capacity: PluginHotReloadEventCapacity,
    quiet_period: PluginHotReloadQuietPeriod,
}

impl PluginHotReloadSettings {
    #[must_use]
    pub const fn new(
        host_queue_capacity: PluginHostQueueCapacity,
        event_capacity: PluginHotReloadEventCapacity,
        quiet_period: PluginHotReloadQuietPeriod,
    ) -> Self {
        Self {
            host_queue_capacity,
            event_capacity,
            quiet_period,
        }
    }
}

/// One terminal outcome for a native file-change attempt.
#[derive(Debug)]
pub enum PluginHotReloadEvent {
    Reloaded(PluginLoadReport),
    Rejected(PluginLoadFailure),
    SourceFailed(PluginSourceLoadError),
    WatchFailed(PluginWatchFailure),
    HostStopped,
}

/// Owned native watcher and asynchronous transactional-reload loop.
pub struct PluginHotReloadService {
    source: PluginSourceFile,
    client: PluginHostClient,
    initial_load: PluginLoadReport,
    events: mpsc::Receiver<PluginHotReloadEvent>,
    cancellation: CancellationToken,
    owner: Option<JoinHandle<()>>,
    watcher: Option<ReadDirectoryChangesWatcher>,
    host: Option<PluginHostService>,
}

impl PluginHotReloadService {
    /// Registers the exact file with the Windows `ReadDirectoryChangesW` backend.
    pub async fn start(
        worker: PathBuf,
        manifest: PluginManifest,
        limits: PluginLimits,
        source: PluginSourceFile,
        settings: PluginHotReloadSettings,
    ) -> Result<Self, PluginHotReloadStartError> {
        let runtime = tokio::runtime::Handle::try_current()
            .map_err(PluginHotReloadStartError::RuntimeUnavailable)?;
        let latch = SignalLatch::default();
        let callback_latch = latch.clone();
        let watched_path = source.path().to_path_buf();
        let mut watcher = ReadDirectoryChangesWatcher::new(
            move |event: notify::Result<Event>| match event {
                Ok(event) if concerns_source(&event, &watched_path) => {
                    callback_latch.push(WatcherSignal::Dirty);
                }
                Ok(_) => {}
                Err(error) => {
                    callback_latch.push(WatcherSignal::Failed(error.to_string().into_boxed_str()));
                }
            },
            Config::default(),
        )
        .map_err(PluginHotReloadStartError::Watch)?;
        watcher
            .watch(source.path(), RecursiveMode::NonRecursive)
            .map_err(PluginHotReloadStartError::Watch)?;

        let program = source
            .load()
            .await
            .map_err(PluginHotReloadStartError::Source)?;
        let host = PluginHostService::start(
            worker,
            manifest,
            limits,
            program,
            settings.host_queue_capacity,
        )
        .await
        .map_err(PluginHotReloadStartError::Host)?;
        let client = host.client();
        let initial_load = host.initial_load().clone();

        let cancellation = CancellationToken::new();
        let (event_sender, events) = mpsc::channel(settings.event_capacity.get());
        let owner_cancellation = cancellation.clone();
        let owner_source = source.clone();
        let owner_client = client.clone();
        let owner = runtime.spawn(run_reload_owner(
            owner_client,
            owner_source,
            settings.quiet_period,
            latch,
            owner_cancellation,
            event_sender,
        ));

        Ok(Self {
            source,
            client,
            initial_load,
            events,
            cancellation,
            owner: Some(owner),
            watcher: Some(watcher),
            host: Some(host),
        })
    }

    #[must_use]
    pub const fn source(&self) -> &PluginSourceFile {
        &self.source
    }

    #[must_use]
    pub fn initial_load(&self) -> &PluginLoadReport {
        &self.initial_load
    }

    #[must_use]
    pub fn client(&self) -> PluginHostClient {
        self.client.clone()
    }

    pub async fn next_event(&mut self) -> Option<PluginHotReloadEvent> {
        self.events.recv().await
    }

    /// Stops native notification delivery and joins the reload owner.
    pub async fn shutdown(mut self) -> Result<(), PluginHotReloadShutdownError> {
        self.cancellation.cancel();
        self.watcher.take();
        let Some(owner) = self.owner.take() else {
            return Ok(());
        };
        owner.await.map_err(PluginHotReloadShutdownError::Owner)?;
        let Some(host) = self.host.take() else {
            return Ok(());
        };
        host.shutdown()
            .await
            .map_err(PluginHotReloadShutdownError::Host)
    }
}

impl Drop for PluginHotReloadService {
    fn drop(&mut self) {
        self.cancellation.cancel();
        self.watcher.take();
        if let Some(owner) = self.owner.take() {
            owner.abort();
        }
    }
}

fn concerns_source(event: &Event, source: &Path) -> bool {
    event.need_rescan() || event.paths.iter().any(|path| path == source)
}

async fn run_reload_owner(
    client: PluginHostClient,
    source: PluginSourceFile,
    quiet_period: PluginHotReloadQuietPeriod,
    latch: SignalLatch,
    cancellation: CancellationToken,
    events: mpsc::Sender<PluginHotReloadEvent>,
) {
    loop {
        let signal = tokio::select! {
            () = cancellation.cancelled() => return,
            signal = latch.next() => signal,
        };
        if let WatcherSignal::Failed(message) = signal {
            let _ = send_event(
                &events,
                &cancellation,
                PluginHotReloadEvent::WatchFailed(PluginWatchFailure::new(message)),
            )
            .await;
            return;
        }
        if !settle_change(&latch, quiet_period, &cancellation, &events).await {
            return;
        }

        let event = match source.load().await {
            Ok(program) => match client.reload(program).await {
                Ok(report) => PluginHotReloadEvent::Reloaded(report),
                Err(PluginReloadError::Rejected(failure)) => {
                    PluginHotReloadEvent::Rejected(failure)
                }
                Err(PluginReloadError::Stopped) => PluginHotReloadEvent::HostStopped,
            },
            Err(error) => PluginHotReloadEvent::SourceFailed(error),
        };
        let host_stopped = matches!(event, PluginHotReloadEvent::HostStopped);
        if !send_event(&events, &cancellation, event).await || host_stopped {
            return;
        }
    }
}

async fn settle_change(
    latch: &SignalLatch,
    quiet_period: PluginHotReloadQuietPeriod,
    cancellation: &CancellationToken,
    events: &mpsc::Sender<PluginHotReloadEvent>,
) -> bool {
    loop {
        let quiet = tokio::time::sleep(quiet_period.get());
        tokio::pin!(quiet);
        tokio::select! {
            () = cancellation.cancelled() => return false,
            () = &mut quiet => return true,
            signal = latch.next() => {
                if let WatcherSignal::Failed(message) = signal {
                    let _ = send_event(
                        events,
                        cancellation,
                        PluginHotReloadEvent::WatchFailed(PluginWatchFailure::new(message)),
                    ).await;
                    return false;
                }
            }
        }
    }
}

async fn send_event(
    events: &mpsc::Sender<PluginHotReloadEvent>,
    cancellation: &CancellationToken,
    event: PluginHotReloadEvent,
) -> bool {
    tokio::select! {
        () = cancellation.cancelled() => false,
        result = events.send(event) => result.is_ok(),
    }
}

#[derive(Clone, Default)]
struct SignalLatch(Arc<SignalLatchInner>);

#[derive(Default)]
struct SignalLatchInner {
    pending: Mutex<Option<WatcherSignal>>,
    wake: Notify,
}

impl SignalLatch {
    fn push(&self, signal: WatcherSignal) {
        let mut pending = self.0.pending.lock();
        match (&*pending, signal) {
            (Some(WatcherSignal::Failed(_)), _)
            | (Some(WatcherSignal::Dirty), WatcherSignal::Dirty) => {}
            (_, signal) => *pending = Some(signal),
        }
        drop(pending);
        self.0.wake.notify_one();
    }

    async fn next(&self) -> WatcherSignal {
        loop {
            let notified = self.0.wake.notified();
            if let Some(signal) = self.0.pending.lock().take() {
                return signal;
            }
            notified.await;
        }
    }
}

enum WatcherSignal {
    Dirty,
    Failed(Box<str>),
}

#[derive(Debug, Error)]
pub enum PluginHotReloadStartError {
    #[error("hot reload requires an active Tokio runtime: {0}")]
    RuntimeUnavailable(TryCurrentError),
    #[error("failed to register native extension source notifications: {0}")]
    Watch(notify::Error),
    #[error("failed to load initial extension source: {0}")]
    Source(PluginSourceLoadError),
    #[error("failed to start extension host: {0}")]
    Host(PluginHostStartError),
}

#[derive(Debug, Error)]
#[error("native extension source watcher failed: {message}")]
pub struct PluginWatchFailure {
    message: Box<str>,
}

impl PluginWatchFailure {
    const fn new(message: Box<str>) -> Self {
        Self { message }
    }
}

#[derive(Debug, Error)]
pub enum PluginHotReloadShutdownError {
    #[error("extension hot-reload owner task failed: {0}")]
    Owner(JoinError),
    #[error("extension host shutdown failed: {0}")]
    Host(PluginHostShutdownError),
}
