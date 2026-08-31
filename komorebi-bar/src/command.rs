use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Mutex;

use komorebi_client::DefaultLayout;
use komorebi_client::Rect;
use komorebi_protocol::BuiltInActionId;
use komorebi_protocol::BuiltInArgument;
use komorebi_protocol::BuiltInArguments;
use komorebi_protocol::BuiltInArgumentsError;
use komorebi_protocol::BuiltInCursorWarpPolicy;
use komorebi_protocol::BuiltInWorkspaceTarget;
use komorebi_protocol::InvocationSubmissionReply;
use komorebi_protocol::RoleHint;
use komorebi_shell::ActionBinding;
use komorebi_shell::ActionDispatchError;
use komorebi_shell::ActionDispatcher;
use komorebi_shell::ActionInvocationError;
use komorebi_shell::SessionLifetime;
use komorebi_shell::ShellSession;
use komorebi_shell::built_in_layout;
use tokio::sync::watch;
use tokio::task::JoinHandle;

#[derive(Clone, Debug, Eq, PartialEq)]
enum BarCommand {
    BuiltIn {
        key: BarCommandKey,
        arguments: BuiltInArguments,
    },
    Binding(ActionBinding),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BarCommandKey {
    MonitorWorkAreaOffset(u64),
    WorkspaceLayout(WorkspaceTarget),
    WorkspaceTiling(WorkspaceTarget),
    WorkspaceMonocle(WorkspaceTarget),
    WorkspaceActiveContainerLock(WorkspaceTarget),
    FocusMonitorWorkspace,
    FocusStackWindow,
    ToggleWorkspaceLayer,
    TogglePause,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DeliveryPolicy {
    Latest,
    Every,
}

impl BarCommandKey {
    const fn action(self) -> BuiltInActionId {
        match self {
            Self::MonitorWorkAreaOffset(_) => BuiltInActionId::SetMonitorWorkAreaOffset,
            Self::WorkspaceLayout(_) => BuiltInActionId::SetMonitorWorkspaceLayout,
            Self::WorkspaceTiling(_) => BuiltInActionId::SetWorkspaceTiling,
            Self::WorkspaceMonocle(_) => BuiltInActionId::SetWorkspaceMonocle,
            Self::WorkspaceActiveContainerLock(_) => {
                BuiltInActionId::SetWorkspaceActiveContainerLock
            }
            Self::FocusMonitorWorkspace => BuiltInActionId::FocusMonitorWorkspace,
            Self::FocusStackWindow => BuiltInActionId::FocusStackWindow,
            Self::ToggleWorkspaceLayer => BuiltInActionId::ToggleWorkspaceLayer,
            Self::TogglePause => BuiltInActionId::TogglePause,
        }
    }

    const fn delivery(self) -> DeliveryPolicy {
        match self {
            Self::ToggleWorkspaceLayer | Self::TogglePause => DeliveryPolicy::Every,
            Self::MonitorWorkAreaOffset(_)
            | Self::WorkspaceLayout(_)
            | Self::WorkspaceTiling(_)
            | Self::WorkspaceMonocle(_)
            | Self::WorkspaceActiveContainerLock(_)
            | Self::FocusMonitorWorkspace
            | Self::FocusStackWindow => DeliveryPolicy::Latest,
        }
    }
}

impl BarCommand {
    const fn delivery(&self) -> DeliveryPolicy {
        match self {
            Self::BuiltIn { key, .. } => key.delivery(),
            Self::Binding(_) => DeliveryPolicy::Every,
        }
    }

    fn coalesces(&self, pending: &Self) -> bool {
        match (self, pending) {
            (Self::BuiltIn { key, .. }, Self::BuiltIn { key: pending, .. }) => key == pending,
            (Self::BuiltIn { .. } | Self::Binding(_), Self::Binding(_))
            | (Self::Binding(_), Self::BuiltIn { .. }) => false,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct WorkspaceTarget {
    monitor: u64,
    workspace: u64,
}

#[derive(Clone, Debug)]
pub struct CommandQueue {
    pending: Arc<Mutex<VecDeque<BarCommand>>>,
    changed: watch::Sender<u64>,
}

impl CommandQueue {
    #[must_use]
    pub fn start() -> (Self, JoinHandle<()>) {
        let pending = Arc::new(Mutex::new(VecDeque::new()));
        let (changed, receiver) = watch::channel(0);
        let actor = tokio::spawn(run(Arc::clone(&pending), receiver));
        (Self { pending, changed }, actor)
    }

    pub fn set_monitor_offset(
        &self,
        monitor: usize,
        offset: Rect,
    ) -> Result<(), CommandQueueError> {
        let monitor =
            u64::try_from(monitor).map_err(|_| CommandQueueError::MonitorIndexOverflow(monitor))?;
        self.send(
            BarCommandKey::MonitorWorkAreaOffset(monitor),
            [
                BuiltInArgument::Monitor(monitor),
                BuiltInArgument::Left(offset.left),
                BuiltInArgument::Top(offset.top),
                BuiltInArgument::Right(offset.right),
                BuiltInArgument::Bottom(offset.bottom),
            ],
        )
    }

    pub fn set_workspace_layout(
        &self,
        monitor: usize,
        workspace: usize,
        layout: DefaultLayout,
    ) -> Result<(), CommandQueueError> {
        let target = WorkspaceTarget::new(monitor, workspace)?;
        self.send(
            BarCommandKey::WorkspaceLayout(target),
            [
                BuiltInArgument::Monitor(target.monitor),
                BuiltInArgument::Index(target.workspace),
                BuiltInArgument::Layout(built_in_layout(layout)),
            ],
        )
    }

    pub fn set_workspace_tiling(
        &self,
        monitor: usize,
        workspace: usize,
        enabled: bool,
    ) -> Result<(), CommandQueueError> {
        let target = WorkspaceTarget::new(monitor, workspace)?;
        self.send(
            BarCommandKey::WorkspaceTiling(target),
            [
                BuiltInArgument::Monitor(target.monitor),
                BuiltInArgument::Index(target.workspace),
                BuiltInArgument::Enabled(enabled),
            ],
        )
    }

    pub fn set_workspace_monocle(
        &self,
        monitor: usize,
        workspace: usize,
        enabled: bool,
    ) -> Result<(), CommandQueueError> {
        let target = WorkspaceTarget::new(monitor, workspace)?;
        self.send(
            BarCommandKey::WorkspaceMonocle(target),
            [
                BuiltInArgument::Monitor(target.monitor),
                BuiltInArgument::Index(target.workspace),
                BuiltInArgument::Enabled(enabled),
            ],
        )
    }

    pub fn set_workspace_active_container_lock(
        &self,
        monitor: usize,
        workspace: usize,
        locked: bool,
    ) -> Result<(), CommandQueueError> {
        let target = WorkspaceTarget::new(monitor, workspace)?;
        self.send(
            BarCommandKey::WorkspaceActiveContainerLock(target),
            [
                BuiltInArgument::Monitor(target.monitor),
                BuiltInArgument::Index(target.workspace),
                BuiltInArgument::Enabled(locked),
            ],
        )
    }

    pub fn focus_monitor_workspace(
        &self,
        monitor: usize,
        workspace: usize,
    ) -> Result<(), CommandQueueError> {
        let target = WorkspaceTarget::new(monitor, workspace)?;
        self.send(
            BarCommandKey::FocusMonitorWorkspace,
            [
                BuiltInArgument::Monitor(target.monitor),
                BuiltInArgument::Index(target.workspace),
                BuiltInArgument::CursorWarp(BuiltInCursorWarpPolicy::PreservePosition),
            ],
        )
    }

    pub fn focus_stack_window(&self, index: usize) -> Result<(), CommandQueueError> {
        let index =
            u64::try_from(index).map_err(|_| CommandQueueError::StackIndexOverflow(index))?;
        self.send(
            BarCommandKey::FocusStackWindow,
            [
                BuiltInArgument::Index(index),
                BuiltInArgument::CursorWarp(BuiltInCursorWarpPolicy::PreservePosition),
            ],
        )
    }

    pub fn toggle_workspace_layer(&self) -> Result<(), CommandQueueError> {
        self.send(
            BarCommandKey::ToggleWorkspaceLayer,
            [
                BuiltInArgument::WorkspaceTarget(BuiltInWorkspaceTarget::MonitorAtCursor),
                BuiltInArgument::CursorWarp(BuiltInCursorWarpPolicy::PreservePosition),
            ],
        )
    }

    pub fn toggle_pause(&self) -> Result<(), CommandQueueError> {
        self.send(BarCommandKey::TogglePause, [])
    }

    pub fn invoke(&self, binding: ActionBinding) -> Result<(), CommandQueueError> {
        self.enqueue(BarCommand::Binding(binding))
    }

    fn send<const N: usize>(
        &self,
        key: BarCommandKey,
        arguments: [BuiltInArgument; N],
    ) -> Result<(), CommandQueueError> {
        let command = BarCommand::BuiltIn {
            key,
            arguments: BuiltInArguments::new(arguments)?,
        };
        self.enqueue(command)
    }

    fn enqueue(&self, command: BarCommand) -> Result<(), CommandQueueError> {
        if self.changed.is_closed() {
            return Err(CommandQueueError::Closed);
        }
        let mut pending = self
            .pending
            .lock()
            .map_err(|_| CommandQueueError::Poisoned)?;
        enqueue(&mut pending, command);
        drop(pending);
        self.changed.send_modify(|revision| {
            *revision = revision.wrapping_add(1);
        });
        Ok(())
    }
}

impl WorkspaceTarget {
    fn new(monitor: usize, workspace: usize) -> Result<Self, CommandQueueError> {
        Ok(Self {
            monitor: u64::try_from(monitor)
                .map_err(|_| CommandQueueError::MonitorIndexOverflow(monitor))?,
            workspace: u64::try_from(workspace)
                .map_err(|_| CommandQueueError::WorkspaceIndexOverflow(workspace))?,
        })
    }
}

fn enqueue(pending: &mut VecDeque<BarCommand>, command: BarCommand) {
    if command.delivery() == DeliveryPolicy::Latest
        && let Some(index) = pending
            .iter()
            .position(|pending| command.coalesces(pending))
    {
        pending.remove(index);
    }
    pending.push_back(command);
}

async fn run(pending: Arc<Mutex<VecDeque<BarCommand>>>, mut changed: watch::Receiver<u64>) {
    let session = match ShellSession::start(RoleHint::OwnerControl, SessionLifetime::Persistent) {
        Ok(session) => session,
        Err(error) => {
            tracing::error!(%error, "could not start bar command session");
            return;
        }
    };
    let dispatcher = session.dispatcher();
    while changed.changed().await.is_ok() {
        let commands = match pending.lock() {
            Ok(mut pending) => pending.drain(..).collect::<Vec<_>>(),
            Err(error) => {
                tracing::error!("bar command mailbox failed: {error}");
                return;
            }
        };
        for command in commands {
            match dispatch(&dispatcher, command).await {
                Ok(
                    InvocationSubmissionReply::Accepted(_) | InvocationSubmissionReply::Retained(_),
                ) => {}
                Ok(InvocationSubmissionReply::Rejected(reason)) => {
                    tracing::error!("bar command was rejected: {reason:?}");
                }
                Err(error) => {
                    tracing::error!("bar command failed: {error}");
                }
            }
        }
    }
    if let Err(error) = session.shutdown().await {
        tracing::error!(%error, "bar command session failed to stop");
    }
}

async fn dispatch(
    dispatcher: &ActionDispatcher,
    command: BarCommand,
) -> Result<InvocationSubmissionReply, CommandDispatchError> {
    let ticket = match command {
        BarCommand::BuiltIn { key, arguments } => {
            dispatcher.invoke_builtin(key.action(), arguments.into_action_arguments())?
        }
        BarCommand::Binding(binding) => dispatcher.invoke_binding(binding)?,
    };
    Ok(ticket.outcome().await?)
}

#[derive(Debug, thiserror::Error)]
enum CommandDispatchError {
    #[error(transparent)]
    Dispatch(#[from] ActionDispatchError),
    #[error(transparent)]
    Invocation(#[from] ActionInvocationError),
}

#[derive(Debug, thiserror::Error)]
pub enum CommandQueueError {
    #[error(transparent)]
    Arguments(#[from] BuiltInArgumentsError),
    #[error("monitor index {0} cannot be represented by the command protocol")]
    MonitorIndexOverflow(usize),
    #[error("workspace index {0} cannot be represented by the command protocol")]
    WorkspaceIndexOverflow(usize),
    #[error("stack index {0} cannot be represented by the command protocol")]
    StackIndexOverflow(usize),
    #[error("bar command actor is closed")]
    Closed,
    #[error("bar command mailbox is poisoned")]
    Poisoned,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn queue_without_actor() -> (
        CommandQueue,
        Arc<Mutex<VecDeque<BarCommand>>>,
        watch::Receiver<u64>,
    ) {
        let pending = Arc::new(Mutex::new(VecDeque::new()));
        let (changed, receiver) = watch::channel(0);
        (
            CommandQueue {
                pending: Arc::clone(&pending),
                changed,
            },
            pending,
            receiver,
        )
    }

    fn command(key: BarCommandKey) -> BarCommand {
        BarCommand::BuiltIn {
            key,
            arguments: BuiltInArguments::default(),
        }
    }

    #[test]
    fn mailbox_keeps_the_latest_offset_for_each_monitor() {
        let mut pending = VecDeque::new();
        enqueue(
            &mut pending,
            command(BarCommandKey::MonitorWorkAreaOffset(0)),
        );
        enqueue(
            &mut pending,
            command(BarCommandKey::MonitorWorkAreaOffset(1)),
        );
        enqueue(
            &mut pending,
            command(BarCommandKey::MonitorWorkAreaOffset(0)),
        );

        assert_eq!(
            pending
                .iter()
                .filter_map(|command| match command {
                    BarCommand::BuiltIn { key, .. } => Some(*key),
                    BarCommand::Binding(_) => None,
                })
                .collect::<Vec<_>>(),
            [
                BarCommandKey::MonitorWorkAreaOffset(1),
                BarCommandKey::MonitorWorkAreaOffset(0),
            ]
        );
    }

    #[test]
    fn mailbox_coalesces_workspace_layout_per_target() {
        let mut pending = VecDeque::new();
        let first = WorkspaceTarget {
            monitor: 0,
            workspace: 0,
        };
        let second = WorkspaceTarget {
            monitor: 0,
            workspace: 1,
        };
        enqueue(&mut pending, command(BarCommandKey::WorkspaceLayout(first)));
        enqueue(
            &mut pending,
            command(BarCommandKey::WorkspaceLayout(second)),
        );
        enqueue(&mut pending, command(BarCommandKey::WorkspaceLayout(first)));

        assert_eq!(
            pending
                .iter()
                .filter_map(|command| match command {
                    BarCommand::BuiltIn { key, .. } => Some(*key),
                    BarCommand::Binding(_) => None,
                })
                .collect::<Vec<_>>(),
            [
                BarCommandKey::WorkspaceLayout(second),
                BarCommandKey::WorkspaceLayout(first),
            ]
        );
    }

    #[test]
    fn mailbox_keeps_only_the_latest_focus_destination() {
        let mut pending = VecDeque::new();
        enqueue(
            &mut pending,
            command(BarCommandKey::MonitorWorkAreaOffset(0)),
        );
        enqueue(&mut pending, command(BarCommandKey::FocusMonitorWorkspace));
        enqueue(&mut pending, command(BarCommandKey::FocusMonitorWorkspace));

        assert_eq!(
            pending
                .iter()
                .filter_map(|command| match command {
                    BarCommand::BuiltIn { key, .. } => Some(*key),
                    BarCommand::Binding(_) => None,
                })
                .collect::<Vec<_>>(),
            [
                BarCommandKey::MonitorWorkAreaOffset(0),
                BarCommandKey::FocusMonitorWorkspace,
            ]
        );
    }

    #[test]
    fn mailbox_preserves_each_toggle_edge() {
        let mut pending = VecDeque::new();
        enqueue(&mut pending, command(BarCommandKey::ToggleWorkspaceLayer));
        enqueue(&mut pending, command(BarCommandKey::ToggleWorkspaceLayer));

        assert_eq!(
            pending
                .iter()
                .filter_map(|command| match command {
                    BarCommand::BuiltIn { key, .. } => Some(*key),
                    BarCommand::Binding(_) => None,
                })
                .collect::<Vec<_>>(),
            [
                BarCommandKey::ToggleWorkspaceLayer,
                BarCommandKey::ToggleWorkspaceLayer,
            ]
        );
    }

    #[test]
    fn pause_control_enqueues_canonical_action() -> Result<(), CommandQueueError> {
        let (queue, pending, _receiver) = queue_without_actor();

        queue.toggle_pause()?;

        let queued = pending.lock().map_err(|_| CommandQueueError::Poisoned)?;
        assert_eq!(
            queued
                .iter()
                .filter_map(|command| match command {
                    BarCommand::BuiltIn { key, arguments } => {
                        Some((key.action(), arguments.clone()))
                    }
                    BarCommand::Binding(_) => None,
                })
                .collect::<Vec<_>>(),
            [(BuiltInActionId::TogglePause, BuiltInArguments::default())]
        );
        Ok(())
    }

    #[test]
    fn configured_pointer_actions_preserve_every_input_edge()
    -> Result<(), Box<dyn std::error::Error>> {
        let (queue, pending, _receiver) = queue_without_actor();
        let binding: ActionBinding = serde_json::from_value(serde_json::json!({
            "action": "toggle-pause"
        }))?;

        queue.invoke(binding.clone())?;
        queue.invoke(binding.clone())?;

        let queued = pending.lock().map_err(|_| CommandQueueError::Poisoned)?;
        assert_eq!(
            queued.iter().collect::<Vec<_>>(),
            [
                &BarCommand::Binding(binding.clone()),
                &BarCommand::Binding(binding)
            ]
        );
        Ok(())
    }

    #[test]
    fn floating_control_enqueues_exact_workspace_tiling_state() -> Result<(), CommandQueueError> {
        let (queue, pending, _receiver) = queue_without_actor();

        queue.set_workspace_tiling(2, 3, false)?;

        let queued = pending.lock().map_err(|_| CommandQueueError::Poisoned)?;
        assert_eq!(
            queued
                .iter()
                .filter_map(|command| match command {
                    BarCommand::BuiltIn { key, arguments } => {
                        Some((key.action(), arguments.clone()))
                    }
                    BarCommand::Binding(_) => None,
                })
                .collect::<Vec<_>>(),
            [(
                BuiltInActionId::SetWorkspaceTiling,
                BuiltInArguments::new([
                    BuiltInArgument::Monitor(2),
                    BuiltInArgument::Index(3),
                    BuiltInArgument::Enabled(false),
                ])?,
            )]
        );
        Ok(())
    }

    #[test]
    fn monocle_control_enqueues_exact_workspace_state() -> Result<(), CommandQueueError> {
        let (queue, pending, _receiver) = queue_without_actor();

        queue.set_workspace_monocle(2, 3, true)?;

        let queued = pending.lock().map_err(|_| CommandQueueError::Poisoned)?;
        assert_eq!(
            queued
                .iter()
                .filter_map(|command| match command {
                    BarCommand::BuiltIn { key, arguments } => {
                        Some((key.action(), arguments.clone()))
                    }
                    BarCommand::Binding(_) => None,
                })
                .collect::<Vec<_>>(),
            [(
                BuiltInActionId::SetWorkspaceMonocle,
                BuiltInArguments::new([
                    BuiltInArgument::Monitor(2),
                    BuiltInArgument::Index(3),
                    BuiltInArgument::Enabled(true),
                ])?,
            )]
        );
        Ok(())
    }

    #[test]
    fn lock_control_enqueues_exact_workspace_active_container_state()
    -> Result<(), CommandQueueError> {
        let (queue, pending, _receiver) = queue_without_actor();

        queue.set_workspace_active_container_lock(2, 3, true)?;

        let queued = pending.lock().map_err(|_| CommandQueueError::Poisoned)?;
        assert_eq!(
            queued
                .iter()
                .filter_map(|command| match command {
                    BarCommand::BuiltIn { key, arguments } => {
                        Some((key.action(), arguments.clone()))
                    }
                    BarCommand::Binding(_) => None,
                })
                .collect::<Vec<_>>(),
            [(
                BuiltInActionId::SetWorkspaceActiveContainerLock,
                BuiltInArguments::new([
                    BuiltInArgument::Monitor(2),
                    BuiltInArgument::Index(3),
                    BuiltInArgument::Enabled(true),
                ])?,
            )]
        );
        Ok(())
    }

    #[tokio::test]
    async fn actor_exits_when_its_owned_queue_closes() -> Result<(), tokio::task::JoinError> {
        let (queue, actor) = CommandQueue::start();
        drop(queue);
        actor.await
    }
}
