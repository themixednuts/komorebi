use std::num::NonZeroU64;
use std::num::NonZeroUsize;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use color_eyre::eyre;
use color_eyre::eyre::OptionExt;
use komorebi_client::ApplicationIdentifier;
use komorebi_client::Axis;
use komorebi_client::CycleDirection;
use komorebi_client::DefaultLayout;
use komorebi_client::FocusFollowsMouseImplementation;
use komorebi_client::HidingBehaviour;
use komorebi_client::MonocleFocusBehaviour;
use komorebi_client::MoveBehaviour;
use komorebi_client::OperationBehaviour;
use komorebi_client::OperationDirection;
use komorebi_client::Sizing;
use komorebi_client::command::BoundedText;
use komorebi_client::command::BuiltInActionId;
use komorebi_client::command::BuiltInArgument;
use komorebi_client::command::BuiltInArguments;
use komorebi_client::command::BuiltInAxis;
use komorebi_client::command::BuiltInCycle;
use komorebi_client::command::BuiltInDirection;
use komorebi_client::command::BuiltInHidingBehaviour;
use komorebi_client::command::BuiltInIdentifier;
use komorebi_client::command::BuiltInImplementation;
use komorebi_client::command::BuiltInLayout;
use komorebi_client::command::BuiltInMonocleBehaviour;
use komorebi_client::command::BuiltInMoveBehaviour;
use komorebi_client::command::BuiltInOperationBehaviour;
use komorebi_client::command::BuiltInSelector;
use komorebi_client::command::BuiltInSizing;
use komorebi_client::command::CommandClient;
use komorebi_client::command::InvocationSubmissionReply;
use komorebi_client::command::RoleHint;
use komorebi_client::command::SessionLifetime;
use komorebi_client::command::WindowsPathInput;

pub(super) async fn invoke_action(
    action: BuiltInActionId,
    arguments: BuiltInArguments,
) -> eyre::Result<()> {
    let mut client =
        CommandClient::connect(RoleHint::OwnerControl, SessionLifetime::OneShot).await?;
    match client
        .invoke_builtin(action, arguments.into_action_arguments())
        .await?
    {
        InvocationSubmissionReply::Accepted(_) | InvocationSubmissionReply::Retained(_) => Ok(()),
        InvocationSubmissionReply::Rejected(reason) => {
            Err(eyre::eyre!("command was rejected: {reason:?}"))
        }
    }
}

pub(super) fn built_in_arguments<const N: usize>(
    arguments: [BuiltInArgument; N],
) -> eyre::Result<BuiltInArguments> {
    Ok(BuiltInArguments::new(arguments)?)
}

pub(super) fn focused_window_arguments() -> eyre::Result<BuiltInArguments> {
    built_in_arguments([BuiltInArgument::Window(BuiltInSelector::FocusedAtExecution)])
}

pub(super) const fn built_in_direction(value: OperationDirection) -> BuiltInDirection {
    match value {
        OperationDirection::Left => BuiltInDirection::Left,
        OperationDirection::Right => BuiltInDirection::Right,
        OperationDirection::Up => BuiltInDirection::Up,
        OperationDirection::Down => BuiltInDirection::Down,
    }
}

pub(super) const fn built_in_axis(value: Axis) -> BuiltInAxis {
    match value {
        Axis::Horizontal => BuiltInAxis::Horizontal,
        Axis::Vertical => BuiltInAxis::Vertical,
        Axis::HorizontalAndVertical => BuiltInAxis::HorizontalAndVertical,
    }
}

pub(super) const fn built_in_cycle(value: CycleDirection) -> BuiltInCycle {
    match value {
        CycleDirection::Previous => BuiltInCycle::Previous,
        CycleDirection::Next => BuiltInCycle::Next,
    }
}

pub(super) const fn built_in_layout(value: DefaultLayout) -> BuiltInLayout {
    match value {
        DefaultLayout::BSP => BuiltInLayout::Bsp,
        DefaultLayout::Columns => BuiltInLayout::Columns,
        DefaultLayout::Rows => BuiltInLayout::Rows,
        DefaultLayout::VerticalStack => BuiltInLayout::VerticalStack,
        DefaultLayout::HorizontalStack => BuiltInLayout::HorizontalStack,
        DefaultLayout::UltrawideVerticalStack => BuiltInLayout::UltrawideVerticalStack,
        DefaultLayout::Grid => BuiltInLayout::Grid,
        DefaultLayout::RightMainVerticalStack => BuiltInLayout::RightMainVerticalStack,
        DefaultLayout::Scrolling => BuiltInLayout::Scrolling,
    }
}

pub(super) const fn built_in_sizing(value: Sizing) -> BuiltInSizing {
    match value {
        Sizing::Increase => BuiltInSizing::Increase,
        Sizing::Decrease => BuiltInSizing::Decrease,
    }
}

pub(super) const fn built_in_implementation(
    value: FocusFollowsMouseImplementation,
) -> BuiltInImplementation {
    match value {
        FocusFollowsMouseImplementation::Komorebi => BuiltInImplementation::Komorebi,
        FocusFollowsMouseImplementation::Windows => BuiltInImplementation::Windows,
    }
}

pub(super) fn built_in_columns(value: NonZeroUsize) -> eyre::Result<NonZeroU64> {
    NonZeroU64::new(value.get().try_into()?).ok_or_eyre("column count became zero")
}

pub(super) fn built_in_text(value: impl Into<Box<str>>) -> eyre::Result<BoundedText> {
    Ok(BoundedText::new(value)?)
}

pub(super) fn built_in_path(value: &Path) -> eyre::Result<WindowsPathInput> {
    Ok(WindowsPathInput::new(
        value.as_os_str().encode_wide().collect::<Vec<_>>(),
    )?)
}

#[allow(deprecated)]
pub(super) const fn built_in_hiding_behaviour(value: HidingBehaviour) -> BuiltInHidingBehaviour {
    match value {
        HidingBehaviour::Hide => BuiltInHidingBehaviour::Hide,
        HidingBehaviour::Minimize => BuiltInHidingBehaviour::Minimize,
        HidingBehaviour::Cloak => BuiltInHidingBehaviour::Cloak,
    }
}

pub(super) const fn built_in_move_behaviour(value: MoveBehaviour) -> BuiltInMoveBehaviour {
    match value {
        MoveBehaviour::Swap => BuiltInMoveBehaviour::Swap,
        MoveBehaviour::Insert => BuiltInMoveBehaviour::Insert,
        MoveBehaviour::NoOp => BuiltInMoveBehaviour::NoOp,
    }
}

pub(super) const fn built_in_monocle_behaviour(
    value: MonocleFocusBehaviour,
) -> BuiltInMonocleBehaviour {
    match value {
        MonocleFocusBehaviour::Cycle => BuiltInMonocleBehaviour::Cycle,
        MonocleFocusBehaviour::NoOp => BuiltInMonocleBehaviour::NoOp,
    }
}

pub(super) const fn built_in_operation_behaviour(
    value: OperationBehaviour,
) -> BuiltInOperationBehaviour {
    match value {
        OperationBehaviour::Op => BuiltInOperationBehaviour::Op,
        OperationBehaviour::NoOp => BuiltInOperationBehaviour::NoOp,
    }
}

pub(super) const fn built_in_identifier(value: ApplicationIdentifier) -> BuiltInIdentifier {
    match value {
        ApplicationIdentifier::Exe => BuiltInIdentifier::Exe,
        ApplicationIdentifier::Class => BuiltInIdentifier::Class,
        ApplicationIdentifier::Title => BuiltInIdentifier::Title,
        ApplicationIdentifier::Path => BuiltInIdentifier::Path,
    }
}
