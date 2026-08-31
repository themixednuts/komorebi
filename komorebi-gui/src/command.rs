use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Mutex;

use komorebi_client::BorderStyle;
use komorebi_client::Rect;
use komorebi_client::Rgb;
use komorebi_client::StackbarLabel;
use komorebi_client::StackbarMode;
use komorebi_client::WindowKind;
use komorebi_client::command::BoundedText;
use komorebi_client::command::BuiltInActionId;
use komorebi_client::command::BuiltInArgument;
use komorebi_client::command::BuiltInArguments;
use komorebi_client::command::BuiltInArgumentsError;
use komorebi_client::command::BuiltInBorderStyle;
use komorebi_client::command::BuiltInStackbarLabel;
use komorebi_client::command::BuiltInStackbarMode;
use komorebi_client::command::BuiltInWindowKind;
use komorebi_client::command::CommandClient;
use komorebi_client::command::InvocationSubmissionReply;
use komorebi_client::command::RoleHint;
use komorebi_client::command::SessionLifetime;
use tokio::sync::watch;
use tokio::task::JoinHandle;

#[derive(Debug)]
struct GuiCommand {
    key: GuiCommandKey,
    action: BuiltInActionId,
    arguments: BuiltInArguments,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GuiCommandKey {
    Singleton(BuiltInActionId),
    MonitorWorkAreaOffset(u64),
    Workspace(WorkspaceCommandKind, WorkspaceTarget),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WorkspaceCommandKind {
    Name,
    ContainerPadding,
    WorkspacePadding,
}

impl WorkspaceCommandKind {
    const fn action(self) -> BuiltInActionId {
        match self {
            Self::Name => BuiltInActionId::SetWorkspaceName,
            Self::ContainerPadding => BuiltInActionId::SetContainerPadding,
            Self::WorkspacePadding => BuiltInActionId::SetWorkspacePadding,
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

    pub fn set_stackbar_mode(&self, mode: StackbarMode) -> Result<(), CommandQueueError> {
        self.send(
            BuiltInActionId::SetStackbarMode,
            [BuiltInArgument::StackbarMode(built_in_stackbar_mode(mode))],
        )
    }

    pub fn set_stackbar_label(&self, label: StackbarLabel) -> Result<(), CommandQueueError> {
        self.send(
            BuiltInActionId::SetStackbarLabel,
            [BuiltInArgument::StackbarLabel(built_in_stackbar_label(
                label,
            ))],
        )
    }

    pub fn set_stackbar_focused_text_colour(&self, colour: Rgb) -> Result<(), CommandQueueError> {
        self.send_colour(BuiltInActionId::SetStackbarFocusedTextColour, colour)
    }

    pub fn set_stackbar_unfocused_text_colour(&self, colour: Rgb) -> Result<(), CommandQueueError> {
        self.send_colour(BuiltInActionId::SetStackbarUnfocusedTextColour, colour)
    }

    pub fn set_stackbar_background_colour(&self, colour: Rgb) -> Result<(), CommandQueueError> {
        self.send_colour(BuiltInActionId::SetStackbarBackgroundColour, colour)
    }

    pub fn set_stackbar_height(&self, height: i32) -> Result<(), CommandQueueError> {
        self.send(
            BuiltInActionId::SetStackbarHeight,
            [BuiltInArgument::Height(height)],
        )
    }

    pub fn set_stackbar_tab_width(&self, width: i32) -> Result<(), CommandQueueError> {
        self.send(
            BuiltInActionId::SetStackbarTabWidth,
            [BuiltInArgument::TabWidth(width)],
        )
    }

    pub fn set_monitor_work_area_offset(
        &self,
        monitor: usize,
        offset: Rect,
    ) -> Result<(), CommandQueueError> {
        let monitor =
            u64::try_from(monitor).map_err(|_| CommandQueueError::MonitorIndexOverflow(monitor))?;
        self.send_with_key(
            GuiCommandKey::MonitorWorkAreaOffset(monitor),
            BuiltInActionId::SetMonitorWorkAreaOffset,
            [
                BuiltInArgument::Monitor(monitor),
                BuiltInArgument::Left(offset.left),
                BuiltInArgument::Top(offset.top),
                BuiltInArgument::Right(offset.right),
                BuiltInArgument::Bottom(offset.bottom),
            ],
        )
    }

    pub fn set_workspace_name(
        &self,
        monitor: usize,
        workspace: usize,
        name: &str,
    ) -> Result<(), CommandQueueError> {
        let target = WorkspaceTarget::new(monitor, workspace)?;
        let name = BoundedText::new(name).map_err(BuiltInArgumentsError::from)?;
        self.send_to_workspace(
            target,
            WorkspaceCommandKind::Name,
            [
                BuiltInArgument::Monitor(target.monitor),
                BuiltInArgument::Index(target.workspace),
                BuiltInArgument::Name(name),
            ],
        )
    }

    pub fn set_container_padding(
        &self,
        monitor: usize,
        workspace: usize,
        size: i32,
    ) -> Result<(), CommandQueueError> {
        self.set_workspace_padding_value(
            WorkspaceCommandKind::ContainerPadding,
            monitor,
            workspace,
            size,
        )
    }

    pub fn set_workspace_padding(
        &self,
        monitor: usize,
        workspace: usize,
        size: i32,
    ) -> Result<(), CommandQueueError> {
        self.set_workspace_padding_value(
            WorkspaceCommandKind::WorkspacePadding,
            monitor,
            workspace,
            size,
        )
    }

    fn set_workspace_padding_value(
        &self,
        kind: WorkspaceCommandKind,
        monitor: usize,
        workspace: usize,
        size: i32,
    ) -> Result<(), CommandQueueError> {
        let target = WorkspaceTarget::new(monitor, workspace)?;
        self.send_to_workspace(
            target,
            kind,
            [
                BuiltInArgument::Monitor(target.monitor),
                BuiltInArgument::Index(target.workspace),
                BuiltInArgument::Size(size),
            ],
        )
    }

    fn send_to_workspace<const N: usize>(
        &self,
        target: WorkspaceTarget,
        kind: WorkspaceCommandKind,
        arguments: [BuiltInArgument; N],
    ) -> Result<(), CommandQueueError> {
        self.send_with_key(
            GuiCommandKey::Workspace(kind, target),
            kind.action(),
            arguments,
        )
    }

    fn send_colour(&self, action: BuiltInActionId, colour: Rgb) -> Result<(), CommandQueueError> {
        self.send(
            action,
            [
                BuiltInArgument::Red(colour.r),
                BuiltInArgument::Green(colour.g),
                BuiltInArgument::Blue(colour.b),
            ],
        )
    }

    fn send<const N: usize>(
        &self,
        action: BuiltInActionId,
        arguments: [BuiltInArgument; N],
    ) -> Result<(), CommandQueueError> {
        self.send_with_key(GuiCommandKey::Singleton(action), action, arguments)
    }

    fn send_with_key<const N: usize>(
        &self,
        key: GuiCommandKey,
        action: BuiltInActionId,
        arguments: [BuiltInArgument; N],
    ) -> Result<(), CommandQueueError> {
        if self.changed.is_closed() {
            return Err(CommandQueueError::Closed);
        }
        let command = GuiCommand {
            key,
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

fn enqueue(pending: &mut VecDeque<GuiCommand>, command: GuiCommand) {
    if let Some(index) = pending
        .iter()
        .position(|pending| pending.key == command.key)
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

const fn built_in_stackbar_mode(value: StackbarMode) -> BuiltInStackbarMode {
    match value {
        StackbarMode::Always => BuiltInStackbarMode::Always,
        StackbarMode::Never => BuiltInStackbarMode::Never,
        StackbarMode::OnStack => BuiltInStackbarMode::OnStack,
    }
}

const fn built_in_stackbar_label(value: StackbarLabel) -> BuiltInStackbarLabel {
    match value {
        StackbarLabel::Process => BuiltInStackbarLabel::Process,
        StackbarLabel::Title => BuiltInStackbarLabel::Title,
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CommandQueueError {
    #[error(transparent)]
    Arguments(#[from] BuiltInArgumentsError),
    #[error("monitor index {0} cannot be represented by the command protocol")]
    MonitorIndexOverflow(usize),
    #[error("workspace index {0} cannot be represented by the command protocol")]
    WorkspaceIndexOverflow(usize),
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
            key: GuiCommandKey::Singleton(action),
            action,
            arguments: BuiltInArguments::default(),
        }
    }

    fn monitor_work_area_command(monitor: u64) -> GuiCommand {
        GuiCommand {
            key: GuiCommandKey::MonitorWorkAreaOffset(monitor),
            action: BuiltInActionId::SetMonitorWorkAreaOffset,
            arguments: BuiltInArguments::default(),
        }
    }

    fn workspace_command(kind: WorkspaceCommandKind, monitor: u64, workspace: u64) -> GuiCommand {
        GuiCommand {
            key: GuiCommandKey::Workspace(kind, WorkspaceTarget { monitor, workspace }),
            action: kind.action(),
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

    #[test]
    fn mailbox_coalesces_work_area_offsets_per_monitor() {
        let mut pending = VecDeque::new();
        enqueue(&mut pending, monitor_work_area_command(0));
        enqueue(&mut pending, monitor_work_area_command(1));
        enqueue(&mut pending, monitor_work_area_command(0));

        assert_eq!(
            pending
                .iter()
                .map(|command| command.key)
                .collect::<Vec<_>>(),
            [
                GuiCommandKey::MonitorWorkAreaOffset(1),
                GuiCommandKey::MonitorWorkAreaOffset(0),
            ]
        );
    }

    #[test]
    fn mailbox_coalesces_each_action_per_workspace() {
        let mut pending = VecDeque::new();
        enqueue(
            &mut pending,
            workspace_command(WorkspaceCommandKind::ContainerPadding, 0, 0),
        );
        enqueue(
            &mut pending,
            workspace_command(WorkspaceCommandKind::ContainerPadding, 0, 1),
        );
        enqueue(
            &mut pending,
            workspace_command(WorkspaceCommandKind::ContainerPadding, 0, 0),
        );

        assert_eq!(
            pending
                .iter()
                .map(|command| command.key)
                .collect::<Vec<_>>(),
            [
                GuiCommandKey::Workspace(
                    WorkspaceCommandKind::ContainerPadding,
                    WorkspaceTarget {
                        monitor: 0,
                        workspace: 1,
                    },
                ),
                GuiCommandKey::Workspace(
                    WorkspaceCommandKind::ContainerPadding,
                    WorkspaceTarget {
                        monitor: 0,
                        workspace: 0,
                    },
                ),
            ]
        );
    }

    #[test]
    fn mailbox_keeps_distinct_workspace_actions_for_the_same_target() {
        let mut pending = VecDeque::new();
        enqueue(
            &mut pending,
            workspace_command(WorkspaceCommandKind::ContainerPadding, 0, 0),
        );
        enqueue(
            &mut pending,
            workspace_command(WorkspaceCommandKind::WorkspacePadding, 0, 0),
        );

        assert_eq!(pending.len(), 2);
    }

    #[tokio::test]
    async fn actor_exits_when_its_owned_queue_closes() -> Result<(), tokio::task::JoinError> {
        let (queue, actor) = CommandQueue::start();
        drop(queue);
        actor.await
    }
}
