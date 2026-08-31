use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
    time::{Duration, Instant},
};

use parking_lot::Mutex;
use rquickjs::{
    AsyncContext, AsyncRuntime, CatchResultExt, Function, Module, Object, function::Async,
};

use crate::{
    module_loader::{ModuleTelemetry, PluginLoader, PluginResolver},
    path_key,
};

#[derive(Clone, Debug)]
pub struct HostConfig {
    pub root: PathBuf,
    pub memory_limit_bytes: usize,
    pub max_stack_bytes: usize,
    pub timeout: Duration,
}

impl HostConfig {
    #[must_use]
    pub fn for_root(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            memory_limit_bytes: 8 * 1024 * 1024,
            max_stack_bytes: 512 * 1024,
            timeout: Duration::from_millis(100),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

impl Direction {
    fn parse(value: &str) -> rquickjs::Result<Self> {
        match value {
            "left" => Ok(Self::Left),
            "right" => Ok(Self::Right),
            "up" => Ok(Self::Up),
            "down" => Ok(Self::Down),
            _ => Err(rquickjs::Error::new_from_js_message(
                "string",
                "Direction",
                format!("unknown direction {value:?}"),
            )),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HostAction {
    Focus(Direction),
}

#[derive(Clone, Debug)]
pub struct PluginRequest {
    pub entry: PathBuf,
    cancellation: CancellationFlag,
}

impl PluginRequest {
    #[must_use]
    pub fn new(entry: impl Into<PathBuf>) -> Self {
        Self {
            entry: entry.into(),
            cancellation: CancellationFlag::new(),
        }
    }

    #[must_use]
    pub fn with_cancellation(mut self, cancellation: CancellationFlag) -> Self {
        self.cancellation = cancellation;
        self
    }
}

#[derive(Clone, Debug, Default)]
pub struct CancellationFlag(Arc<AtomicBool>);

impl CancellationFlag {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

#[derive(Clone, Debug)]
pub struct ExecutionReport {
    pub actions: Vec<HostAction>,
    pub elapsed: Duration,
    pub heap_bytes: usize,
    pub transformed_modules: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigureError {
    #[error("canonicalize plugin root {path}")]
    Canonicalize {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum ExecuteError {
    #[error("canonicalize plugin entry {path}")]
    CanonicalizeEntry {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("plugin entry escapes the configured root: {0}")]
    EntryOutsideRoot(PathBuf),
    #[error("create QuickJS runtime: {0}")]
    CreateRuntime(rquickjs::Error),
    #[error("create QuickJS context: {0}")]
    CreateContext(rquickjs::Error),
    #[error("execute plugin: {0}")]
    JavaScript(String),
    #[error("plugin exceeded its {limit:?} execution deadline after {elapsed:?}")]
    TimedOut { limit: Duration, elapsed: Duration },
    #[error("plugin execution was cancelled")]
    Cancelled,
    #[error("QuickJS reported a negative memory size: {0}")]
    InvalidMemoryUsage(i64),
}

pub struct Unconfigured;

pub struct Ready {
    config: HostConfig,
}

pub struct PluginHost<State> {
    state: State,
}

impl PluginHost<Unconfigured> {
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: Unconfigured,
        }
    }

    /// Applies resource policy and resolves the root directory.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigureError`] when the plugin root cannot be canonicalized.
    pub fn configure(self, mut config: HostConfig) -> Result<PluginHost<Ready>, ConfigureError> {
        config.root =
            config
                .root
                .canonicalize()
                .map_err(|source| ConfigureError::Canonicalize {
                    path: config.root.clone(),
                    source,
                })?;
        Ok(PluginHost {
            state: Ready { config },
        })
    }
}

impl Default for PluginHost<Unconfigured> {
    fn default() -> Self {
        Self::new()
    }
}

impl PluginHost<Ready> {
    /// Executes one plugin entry module under the configured resource policy.
    ///
    /// # Errors
    ///
    /// Returns [`ExecuteError`] for path, runtime, resource-limit, cancellation, and script
    /// failures.
    pub async fn execute(&self, request: PluginRequest) -> Result<ExecutionReport, ExecuteError> {
        let entry =
            request
                .entry
                .canonicalize()
                .map_err(|source| ExecuteError::CanonicalizeEntry {
                    path: request.entry.clone(),
                    source,
                })?;
        if !entry.starts_with(&self.state.config.root) {
            return Err(ExecuteError::EntryOutsideRoot(entry));
        }

        let started = Instant::now();
        let actions = Arc::new(Mutex::new(Vec::new()));
        let callback_actions = Arc::clone(&actions);
        let telemetry = Arc::new(ModuleTelemetry::default());
        let runtime = AsyncRuntime::new().map_err(ExecuteError::CreateRuntime)?;
        runtime
            .set_memory_limit(self.state.config.memory_limit_bytes)
            .await;
        runtime
            .set_max_stack_size(self.state.config.max_stack_bytes)
            .await;
        let deadline = started + self.state.config.timeout;
        let interrupt_reason = Arc::new(AtomicU8::new(0));
        let handler_reason = Arc::clone(&interrupt_reason);
        let cancellation = request.cancellation;
        runtime
            .set_interrupt_handler(Some(Box::new(move || {
                let reason = if cancellation.is_cancelled() {
                    2
                } else if Instant::now() >= deadline {
                    1
                } else {
                    return false;
                };
                handler_reason.store(reason, Ordering::Release);
                true
            })))
            .await;
        runtime
            .set_loader(
                PluginResolver::new(self.state.config.root.clone()),
                PluginLoader::new(Arc::clone(&telemetry)),
            )
            .await;
        let context = AsyncContext::full(&runtime)
            .await
            .map_err(ExecuteError::CreateContext)?;
        let entry_key = path_key::encode(&entry);

        let execution = context
            .async_with(async move |ctx| {
                let focus = Function::new(
                    ctx.clone(),
                    Async(move |value: String| {
                        let callback_actions = Arc::clone(&callback_actions);
                        async move {
                            tokio::task::yield_now().await;
                            callback_actions
                                .lock()
                                .push(HostAction::Focus(Direction::parse(&value)?));
                            Ok::<_, rquickjs::Error>(())
                        }
                    }),
                )
                .map_err(|error| error.to_string())?;
                ctx.globals()
                    .set("__komorebi_focus", focus)
                    .map_err(|error| error.to_string())?;
                let promise = Module::import(&ctx, entry_key)
                    .catch(&ctx)
                    .map_err(|error| error.to_string())?;
                let _: Object = promise
                    .into_future()
                    .await
                    .catch(&ctx)
                    .map_err(|error| error.to_string())?;
                Ok::<_, String>(())
            })
            .await;
        if let Err(message) = execution {
            return Err(match interrupt_reason.load(Ordering::Acquire) {
                1 => ExecuteError::TimedOut {
                    limit: self.state.config.timeout,
                    elapsed: started.elapsed(),
                },
                2 => ExecuteError::Cancelled,
                _ => ExecuteError::JavaScript(telemetry.remap_diagnostic(&message)),
            });
        }

        let memory = runtime.memory_usage().await;
        let heap_bytes = usize::try_from(memory.memory_used_size)
            .map_err(|_| ExecuteError::InvalidMemoryUsage(memory.memory_used_size))?;
        Ok(ExecutionReport {
            actions: actions.lock().clone(),
            elapsed: started.elapsed(),
            heap_bytes,
            transformed_modules: telemetry.transformed_modules(),
        })
    }
}
