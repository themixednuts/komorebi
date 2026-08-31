use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Mutex;

use komorebi_client::DefaultLayout;
use komorebi_client::Rect;
use komorebi_client::command::BuiltInActionId;
use komorebi_client::command::BuiltInArgument;
use komorebi_client::command::BuiltInArguments;
use komorebi_client::command::BuiltInArgumentsError;
use komorebi_client::command::BuiltInCursorWarpPolicy;
use komorebi_client::command::CommandClient;
use komorebi_client::command::InvocationSubmissionReply;
use komorebi_client::command::RoleHint;
use komorebi_client::command::SessionLifetime;
use komorebi_client::command::built_in_layout;
use tokio::sync::watch;
use tokio::task::JoinHandle;

#[derive(Debug)]
struct BarCommand {
    key: BarCommandKey,
    arguments: BuiltInArguments,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BarCommandKey {
    MonitorWorkAreaOffset(u64),
    WorkspaceLayout(WorkspaceTarget),
    FocusMonitorWorkspace,
}

impl BarCommandKey {
    const fn action(self) -> BuiltInActionId {
        match self {
            Self::MonitorWorkAreaOffset(_) => BuiltInActionId::SetMonitorWorkAreaOffset,
            Self::WorkspaceLayout(_) => BuiltInActionId::SetMonitorWorkspaceLayout,
            Self::FocusMonitorWorkspace => BuiltInActionId::FocusMonitorWorkspace,
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

    fn send<const N: usize>(
        &self,
        key: BarCommandKey,
        arguments: [BuiltInArgument; N],
    ) -> Result<(), CommandQueueError> {
        if self.changed.is_closed() {
            return Err(CommandQueueError::Closed);
        }
        let command = BarCommand {
            key,
            arguments: BuiltInArguments::new(arguments)?,
        };
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
    if let Some(index) = pending
        .iter()
        .position(|pending| pending.key == command.key)
    {
        pending.remove(index);
    }
    pending.push_back(command);
}

async fn run(pending: Arc<Mutex<VecDeque<BarCommand>>>, mut changed: watch::Receiver<u64>) {
    let mut client = None;
    while changed.changed().await.is_ok() {
        let commands = match pending.lock() {
            Ok(mut pending) => pending.drain(..).collect::<Vec<_>>(),
            Err(error) => {
                tracing::error!("bar command mailbox failed: {error}");
                return;
            }
        };
        for command in commands {
            match dispatch(&mut client, command).await {
                Ok(
                    InvocationSubmissionReply::Accepted(_) | InvocationSubmissionReply::Retained(_),
                ) => {}
                Ok(InvocationSubmissionReply::Rejected(reason)) => {
                    tracing::error!("bar command was rejected: {reason:?}");
                }
                Err(error) => {
                    client = None;
                    tracing::error!("bar command failed: {error}");
                }
            }
        }
    }
}

async fn dispatch(
    current: &mut Option<CommandClient>,
    command: BarCommand,
) -> Result<InvocationSubmissionReply, komorebi_client::command::CommandClientError> {
    if let Some(client) = current.as_mut() {
        client.refresh_catalog().await?;
        return client
            .invoke_builtin(
                command.key.action(),
                command.arguments.into_action_arguments(),
            )
            .await;
    }

    let mut client =
        CommandClient::connect(RoleHint::OwnerControl, SessionLifetime::Persistent).await?;
    let reply = client
        .invoke_builtin(
            command.key.action(),
            command.arguments.into_action_arguments(),
        )
        .await?;
    *current = Some(client);
    Ok(reply)
}

#[derive(Debug, thiserror::Error)]
pub enum CommandQueueError {
    #[error(transparent)]
    Arguments(#[from] BuiltInArgumentsError),
    #[error("monitor index {0} cannot be represented by the command protocol")]
    MonitorIndexOverflow(usize),
    #[error("workspace index {0} cannot be represented by the command protocol")]
    WorkspaceIndexOverflow(usize),
    #[error("bar command actor is closed")]
    Closed,
    #[error("bar command mailbox is poisoned")]
    Poisoned,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(key: BarCommandKey) -> BarCommand {
        BarCommand {
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
                .map(|command| command.key)
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
                .map(|command| command.key)
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
                .map(|command| command.key)
                .collect::<Vec<_>>(),
            [
                BarCommandKey::MonitorWorkAreaOffset(0),
                BarCommandKey::FocusMonitorWorkspace,
            ]
        );
    }

    #[tokio::test]
    async fn actor_exits_when_its_owned_queue_closes() -> Result<(), tokio::task::JoinError> {
        let (queue, actor) = CommandQueue::start();
        drop(queue);
        actor.await
    }
}
