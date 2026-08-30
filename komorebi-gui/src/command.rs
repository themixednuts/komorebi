use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Mutex;

use komorebi_client::BorderStyle;
use komorebi_client::Rgb;
use komorebi_client::WindowKind;
use komorebi_client::command::BuiltInActionId;
use komorebi_client::command::BuiltInArgument;
use komorebi_client::command::BuiltInArguments;
use komorebi_client::command::BuiltInArgumentsError;
use komorebi_client::command::BuiltInBorderStyle;
use komorebi_client::command::BuiltInWindowKind;
use komorebi_client::command::CommandClient;
use komorebi_client::command::InvocationSubmissionReply;
use komorebi_client::command::RoleHint;
use komorebi_client::command::SessionLifetime;
use tokio::sync::watch;
use tokio::task::JoinHandle;

#[derive(Debug)]
struct GuiCommand {
    action: BuiltInActionId,
    arguments: BuiltInArguments,
}

#[derive(Clone, Debug)]
pub struct CommandQueue {
    pending: Arc<Mutex<VecDeque<GuiCommand>>>,
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

    pub fn set_border_enabled(&self, enabled: bool) -> Result<(), CommandQueueError> {
        self.send(
            BuiltInActionId::SetBorderEnabled,
            [BuiltInArgument::Enabled(enabled)],
        )
    }

    pub fn set_border_colour(
        &self,
        window_kind: WindowKind,
        colour: Rgb,
    ) -> Result<(), CommandQueueError> {
        self.send(
            BuiltInActionId::SetBorderColour,
            [
                BuiltInArgument::WindowKind(built_in_window_kind(window_kind)),
                BuiltInArgument::Red(colour.r),
                BuiltInArgument::Green(colour.g),
                BuiltInArgument::Blue(colour.b),
            ],
        )
    }

    pub fn set_border_width(&self, width: i32) -> Result<(), CommandQueueError> {
        self.send(
            BuiltInActionId::SetBorderWidth,
            [BuiltInArgument::Width(width)],
        )
    }

    pub fn set_border_offset(&self, offset: i32) -> Result<(), CommandQueueError> {
        self.send(
            BuiltInActionId::SetBorderOffset,
            [BuiltInArgument::Offset(offset)],
        )
    }

    pub fn set_border_style(&self, style: BorderStyle) -> Result<(), CommandQueueError> {
        self.send(
            BuiltInActionId::SetBorderStyle,
            [BuiltInArgument::BorderStyle(built_in_border_style(style))],
        )
    }

    fn send<const N: usize>(
        &self,
        action: BuiltInActionId,
        arguments: [BuiltInArgument; N],
    ) -> Result<(), CommandQueueError> {
        if self.changed.is_closed() {
            return Err(CommandQueueError::Closed);
        }
        let command = GuiCommand {
            action,
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

fn enqueue(pending: &mut VecDeque<GuiCommand>, command: GuiCommand) {
    if let Some(index) = pending
        .iter()
        .position(|pending| pending.action == command.action)
    {
        pending.remove(index);
    }
    pending.push_back(command);
}

async fn run(pending: Arc<Mutex<VecDeque<GuiCommand>>>, mut changed: watch::Receiver<u64>) {
    let mut client = None;
    while changed.changed().await.is_ok() {
        let commands = match pending.lock() {
            Ok(mut pending) => pending.drain(..).collect::<Vec<_>>(),
            Err(error) => {
                eprintln!("GUI command mailbox failed: {error}");
                return;
            }
        };
        for command in commands {
            match dispatch(&mut client, command).await {
                Ok(
                    InvocationSubmissionReply::Accepted(_) | InvocationSubmissionReply::Retained(_),
                ) => {}
                Ok(InvocationSubmissionReply::Rejected(reason)) => {
                    eprintln!("GUI command was rejected: {reason:?}");
                }
                Err(error) => {
                    client = None;
                    eprintln!("GUI command failed: {error}");
                }
            }
        }
    }
}

async fn dispatch(
    current: &mut Option<CommandClient>,
    command: GuiCommand,
) -> Result<InvocationSubmissionReply, komorebi_client::command::CommandClientError> {
    if let Some(client) = current.as_mut() {
        client.refresh_catalog().await?;
        return client
            .invoke_builtin(command.action, command.arguments.into_action_arguments())
            .await;
    }

    let mut client =
        CommandClient::connect(RoleHint::OwnerControl, SessionLifetime::Persistent).await?;
    let reply = client
        .invoke_builtin(command.action, command.arguments.into_action_arguments())
        .await?;
    *current = Some(client);
    Ok(reply)
}

const fn built_in_window_kind(value: WindowKind) -> BuiltInWindowKind {
    match value {
        WindowKind::Single => BuiltInWindowKind::Single,
        WindowKind::Stack => BuiltInWindowKind::Stack,
        WindowKind::Monocle => BuiltInWindowKind::Monocle,
        WindowKind::Unfocused => BuiltInWindowKind::Unfocused,
        WindowKind::UnfocusedLocked => BuiltInWindowKind::UnfocusedLocked,
        WindowKind::Floating => BuiltInWindowKind::Floating,
    }
}

const fn built_in_border_style(value: BorderStyle) -> BuiltInBorderStyle {
    match value {
        BorderStyle::System => BuiltInBorderStyle::System,
        BorderStyle::Rounded => BuiltInBorderStyle::Rounded,
        BorderStyle::Square => BuiltInBorderStyle::Square,
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CommandQueueError {
    #[error(transparent)]
    Arguments(#[from] BuiltInArgumentsError),
    #[error("gui command actor is closed")]
    Closed,
    #[error("gui command mailbox is poisoned")]
    Poisoned,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(action: BuiltInActionId) -> GuiCommand {
        GuiCommand {
            action,
            arguments: BuiltInArguments::default(),
        }
    }

    #[test]
    fn mailbox_keeps_only_the_latest_value_per_action_in_user_order() {
        let mut pending = VecDeque::new();
        enqueue(&mut pending, command(BuiltInActionId::SetBorderWidth));
        enqueue(&mut pending, command(BuiltInActionId::SetBorderOffset));
        enqueue(&mut pending, command(BuiltInActionId::SetBorderWidth));

        assert_eq!(
            pending
                .iter()
                .map(|command| command.action)
                .collect::<Vec<_>>(),
            [
                BuiltInActionId::SetBorderOffset,
                BuiltInActionId::SetBorderWidth,
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
