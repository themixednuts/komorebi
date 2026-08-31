use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::Mutex;

use komorebi_client::Rect;
use komorebi_client::command::BuiltInActionId;
use komorebi_client::command::BuiltInArgument;
use komorebi_client::command::BuiltInArguments;
use komorebi_client::command::BuiltInArgumentsError;
use komorebi_client::command::CommandClient;
use komorebi_client::command::InvocationSubmissionReply;
use komorebi_client::command::RoleHint;
use komorebi_client::command::SessionLifetime;
use tokio::sync::watch;
use tokio::task::JoinHandle;

#[derive(Debug)]
struct WorkAreaCommand {
    monitor: u64,
    offset: Rect,
}

#[derive(Clone, Debug)]
pub struct WorkAreaCommandQueue {
    pending: Arc<Mutex<BTreeMap<u64, WorkAreaCommand>>>,
    changed: watch::Sender<u64>,
}

impl WorkAreaCommandQueue {
    #[must_use]
    pub fn start() -> (Self, JoinHandle<()>) {
        let pending = Arc::new(Mutex::new(BTreeMap::new()));
        let (changed, receiver) = watch::channel(0);
        let actor = tokio::spawn(run(Arc::clone(&pending), receiver));
        (Self { pending, changed }, actor)
    }

    pub fn set_monitor_offset(
        &self,
        monitor: usize,
        offset: Rect,
    ) -> Result<(), WorkAreaCommandQueueError> {
        if self.changed.is_closed() {
            return Err(WorkAreaCommandQueueError::Closed);
        }
        let monitor = u64::try_from(monitor)
            .map_err(|_| WorkAreaCommandQueueError::MonitorIndexOverflow(monitor))?;
        let mut pending = self
            .pending
            .lock()
            .map_err(|_| WorkAreaCommandQueueError::Poisoned)?;
        enqueue(&mut pending, WorkAreaCommand { monitor, offset });
        drop(pending);
        self.changed.send_modify(|revision| {
            *revision = revision.wrapping_add(1);
        });
        Ok(())
    }
}

fn enqueue(pending: &mut BTreeMap<u64, WorkAreaCommand>, command: WorkAreaCommand) {
    pending.insert(command.monitor, command);
}

async fn run(
    pending: Arc<Mutex<BTreeMap<u64, WorkAreaCommand>>>,
    mut changed: watch::Receiver<u64>,
) {
    let mut client = None;
    while changed.changed().await.is_ok() {
        let commands = match pending.lock() {
            Ok(mut pending) => std::mem::take(&mut *pending).into_values(),
            Err(error) => {
                tracing::error!("bar work-area command mailbox failed: {error}");
                return;
            }
        };
        for command in commands {
            match dispatch(&mut client, command).await {
                Ok(
                    InvocationSubmissionReply::Accepted(_) | InvocationSubmissionReply::Retained(_),
                ) => {}
                Ok(InvocationSubmissionReply::Rejected(reason)) => {
                    tracing::error!("bar work-area command was rejected: {reason:?}");
                }
                Err(error) => {
                    client = None;
                    tracing::error!("bar work-area command failed: {error}");
                }
            }
        }
    }
}

async fn dispatch(
    current: &mut Option<CommandClient>,
    command: WorkAreaCommand,
) -> Result<InvocationSubmissionReply, WorkAreaCommandError> {
    let arguments = BuiltInArguments::new([
        BuiltInArgument::Monitor(command.monitor),
        BuiltInArgument::Left(command.offset.left),
        BuiltInArgument::Top(command.offset.top),
        BuiltInArgument::Right(command.offset.right),
        BuiltInArgument::Bottom(command.offset.bottom),
    ])?
    .into_action_arguments();
    if let Some(client) = current.as_mut() {
        client.refresh_catalog().await?;
        return Ok(client
            .invoke_builtin(BuiltInActionId::SetMonitorWorkAreaOffset, arguments)
            .await?);
    }

    let mut client =
        CommandClient::connect(RoleHint::OwnerControl, SessionLifetime::Persistent).await?;
    let reply = client
        .invoke_builtin(BuiltInActionId::SetMonitorWorkAreaOffset, arguments)
        .await?;
    *current = Some(client);
    Ok(reply)
}

#[derive(Debug, thiserror::Error)]
enum WorkAreaCommandError {
    #[error(transparent)]
    Arguments(#[from] BuiltInArgumentsError),
    #[error(transparent)]
    Client(#[from] komorebi_client::command::CommandClientError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum WorkAreaCommandQueueError {
    #[error("monitor index {0} cannot be represented by the command protocol")]
    MonitorIndexOverflow(usize),
    #[error("bar work-area command actor is closed")]
    Closed,
    #[error("bar work-area command mailbox is poisoned")]
    Poisoned,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mailbox_keeps_the_latest_offset_for_each_monitor() {
        let mut pending = BTreeMap::new();
        enqueue(
            &mut pending,
            WorkAreaCommand {
                monitor: 0,
                offset: Rect::default(),
            },
        );
        enqueue(
            &mut pending,
            WorkAreaCommand {
                monitor: 1,
                offset: Rect::default(),
            },
        );
        let latest = Rect {
            left: -1,
            top: 2,
            right: 3,
            bottom: -4,
        };
        enqueue(
            &mut pending,
            WorkAreaCommand {
                monitor: 0,
                offset: latest,
            },
        );

        assert_eq!(pending.len(), 2);
        assert_eq!(pending.get(&0).map(|command| command.offset), Some(latest));
    }

    #[tokio::test]
    async fn actor_exits_when_its_owned_queue_closes() -> Result<(), tokio::task::JoinError> {
        let (queue, actor) = WorkAreaCommandQueue::start();
        drop(queue);
        actor.await
    }
}
