use std::num::NonZeroU64;
use std::num::NonZeroUsize;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use color_eyre::eyre;
use color_eyre::eyre::OptionExt;
use komorebi_client::AnimationPrefix;
use komorebi_client::AnimationStyle;
use komorebi_client::ApplicationIdentifier;
use komorebi_client::Axis;
use komorebi_client::BorderImplementation;
use komorebi_client::BorderStyle;
use komorebi_client::CycleDirection;
use komorebi_client::FocusFollowsMouseImplementation;
use komorebi_client::HidingBehaviour;
use komorebi_client::MonocleFocusBehaviour;
use komorebi_client::MoveBehaviour;
use komorebi_client::OperationBehaviour;
use komorebi_client::OperationDirection;
use komorebi_client::Sizing;
use komorebi_client::StackbarMode;
use komorebi_client::WindowKind;
use komorebi_protocol::BoundedText;
use komorebi_protocol::BuiltInActionId;
use komorebi_protocol::BuiltInAnimationPrefix;
use komorebi_protocol::BuiltInAnimationStyle;
use komorebi_protocol::BuiltInArgument;
use komorebi_protocol::BuiltInArguments;
use komorebi_protocol::BuiltInAxis;
use komorebi_protocol::BuiltInBorderImplementation;
use komorebi_protocol::BuiltInBorderStyle;
use komorebi_protocol::BuiltInCursorWarpPolicy;
use komorebi_protocol::BuiltInCycle;
use komorebi_protocol::BuiltInDirection;
use komorebi_protocol::BuiltInHidingBehaviour;
use komorebi_protocol::BuiltInIdentifier;
use komorebi_protocol::BuiltInImplementation;
use komorebi_protocol::BuiltInMonocleBehaviour;
use komorebi_protocol::BuiltInMoveBehaviour;
use komorebi_protocol::BuiltInNamedAnimationStyle;
use komorebi_protocol::BuiltInOperationBehaviour;
use komorebi_protocol::BuiltInSelector;
use komorebi_protocol::BuiltInSizing;
use komorebi_protocol::BuiltInStackbarMode;
use komorebi_protocol::BuiltInWindowKind;
use komorebi_protocol::BuiltInWorkspaceTarget;
use komorebi_protocol::InvocationSubmissionReply;
use komorebi_protocol::RoleHint;
use komorebi_protocol::WindowsPathInput;
use komorebi_shell::SessionLifetime;
use komorebi_shell::ShellSession;

pub(super) async fn invoke_action(
    action: BuiltInActionId,
    arguments: BuiltInArguments,
) -> eyre::Result<()> {
    let session = ShellSession::start(RoleHint::OwnerControl, SessionLifetime::OneShot)?;
    let outcome = session
        .dispatcher()
        .invoke_builtin(action, arguments.into_action_arguments())?
        .outcome()
        .await;
    session.shutdown().await?;
    match outcome? {
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

pub(super) fn focus_monitor_workspace_arguments(
    monitor: usize,
    workspace: usize,
) -> eyre::Result<BuiltInArguments> {
    built_in_arguments([
        BuiltInArgument::Monitor(monitor.try_into()?),
        BuiltInArgument::Index(workspace.try_into()?),
        BuiltInArgument::CursorWarp(BuiltInCursorWarpPolicy::FollowConfiguration),
    ])
}

pub(super) fn focus_stack_window_arguments(index: usize) -> eyre::Result<BuiltInArguments> {
    built_in_arguments([
        BuiltInArgument::Index(index.try_into()?),
        BuiltInArgument::CursorWarp(BuiltInCursorWarpPolicy::FollowConfiguration),
    ])
}

pub(super) fn toggle_workspace_layer_arguments() -> eyre::Result<BuiltInArguments> {
    built_in_arguments([
        BuiltInArgument::WorkspaceTarget(BuiltInWorkspaceTarget::FocusedAtExecution),
        BuiltInArgument::CursorWarp(BuiltInCursorWarpPolicy::FollowConfiguration),
    ])
}

pub(super) fn work_area_arguments<const N: usize>(
    target: [BuiltInArgument; N],
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
) -> eyre::Result<BuiltInArguments> {
    Ok(BuiltInArguments::new(target.into_iter().chain([
        BuiltInArgument::Left(left),
        BuiltInArgument::Top(top),
        BuiltInArgument::Right(right),
        BuiltInArgument::Bottom(bottom),
    ]))?)
}

pub(super) fn scoped_animation_arguments(
    value: BuiltInArgument,
    prefix: Option<AnimationPrefix>,
) -> eyre::Result<BuiltInArguments> {
    let mut arguments = vec![value];
    if let Some(prefix) = prefix {
        arguments.push(BuiltInArgument::AnimationPrefix(built_in_animation_prefix(
            prefix,
        )));
    }
    Ok(BuiltInArguments::new(arguments)?)
}

pub(super) const fn built_in_animation_prefix(value: AnimationPrefix) -> BuiltInAnimationPrefix {
    match value {
        AnimationPrefix::Movement => BuiltInAnimationPrefix::Movement,
        AnimationPrefix::Transparency => BuiltInAnimationPrefix::Transparency,
    }
}

pub(super) fn built_in_animation_style(
    value: AnimationStyle,
) -> eyre::Result<BuiltInAnimationStyle> {
    use BuiltInNamedAnimationStyle as S;

    let style = match value {
        AnimationStyle::Linear => S::Linear,
        AnimationStyle::EaseInSine => S::EaseInSine,
        AnimationStyle::EaseOutSine => S::EaseOutSine,
        AnimationStyle::EaseInOutSine => S::EaseInOutSine,
        AnimationStyle::EaseInQuad => S::EaseInQuad,
        AnimationStyle::EaseOutQuad => S::EaseOutQuad,
        AnimationStyle::EaseInOutQuad => S::EaseInOutQuad,
        AnimationStyle::EaseInCubic => S::EaseInCubic,
        AnimationStyle::EaseOutCubic => S::EaseOutCubic,
        AnimationStyle::EaseInOutCubic => S::EaseInOutCubic,
        AnimationStyle::EaseInQuart => S::EaseInQuart,
        AnimationStyle::EaseOutQuart => S::EaseOutQuart,
        AnimationStyle::EaseInOutQuart => S::EaseInOutQuart,
        AnimationStyle::EaseInQuint => S::EaseInQuint,
        AnimationStyle::EaseOutQuint => S::EaseOutQuint,
        AnimationStyle::EaseInOutQuint => S::EaseInOutQuint,
        AnimationStyle::EaseInExpo => S::EaseInExpo,
        AnimationStyle::EaseOutExpo => S::EaseOutExpo,
        AnimationStyle::EaseInOutExpo => S::EaseInOutExpo,
        AnimationStyle::EaseInCirc => S::EaseInCirc,
        AnimationStyle::EaseOutCirc => S::EaseOutCirc,
        AnimationStyle::EaseInOutCirc => S::EaseInOutCirc,
        AnimationStyle::EaseInBack => S::EaseInBack,
        AnimationStyle::EaseOutBack => S::EaseOutBack,
        AnimationStyle::EaseInOutBack => S::EaseInOutBack,
        AnimationStyle::EaseInElastic => S::EaseInElastic,
        AnimationStyle::EaseOutElastic => S::EaseOutElastic,
        AnimationStyle::EaseInOutElastic => S::EaseInOutElastic,
        AnimationStyle::EaseInBounce => S::EaseInBounce,
        AnimationStyle::EaseOutBounce => S::EaseOutBounce,
        AnimationStyle::EaseInOutBounce => S::EaseInOutBounce,
        AnimationStyle::CubicBezier(..) => {
            eyre::bail!(
                "the CLI accepts named animation styles; exact cubic bezier values are available through the typed action API"
            )
        }
    };
    Ok(BuiltInAnimationStyle::Named(style))
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

pub(super) const fn built_in_window_kind(value: WindowKind) -> BuiltInWindowKind {
    match value {
        WindowKind::Single => BuiltInWindowKind::Single,
        WindowKind::Stack => BuiltInWindowKind::Stack,
        WindowKind::Monocle => BuiltInWindowKind::Monocle,
        WindowKind::Unfocused => BuiltInWindowKind::Unfocused,
        WindowKind::UnfocusedLocked => BuiltInWindowKind::UnfocusedLocked,
        WindowKind::Floating => BuiltInWindowKind::Floating,
    }
}

pub(super) const fn built_in_border_style(value: BorderStyle) -> BuiltInBorderStyle {
    match value {
        BorderStyle::System => BuiltInBorderStyle::System,
        BorderStyle::Rounded => BuiltInBorderStyle::Rounded,
        BorderStyle::Square => BuiltInBorderStyle::Square,
    }
}

pub(super) const fn built_in_border_implementation(
    value: BorderImplementation,
) -> BuiltInBorderImplementation {
    match value {
        BorderImplementation::Komorebi => BuiltInBorderImplementation::Komorebi,
        BorderImplementation::Windows => BuiltInBorderImplementation::Windows,
    }
}

pub(super) const fn built_in_stackbar_mode(value: StackbarMode) -> BuiltInStackbarMode {
    match value {
        StackbarMode::Always => BuiltInStackbarMode::Always,
        StackbarMode::Never => BuiltInStackbarMode::Never,
        StackbarMode::OnStack => BuiltInStackbarMode::OnStack,
    }
}

pub(super) const fn built_in_cycle(value: CycleDirection) -> BuiltInCycle {
    match value {
        CycleDirection::Previous => BuiltInCycle::Previous,
        CycleDirection::Next => BuiltInCycle::Next,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focus_monitor_workspace_uses_configured_cursor_policy()
    -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(
            focus_monitor_workspace_arguments(2, 3)?,
            built_in_arguments([
                BuiltInArgument::Monitor(2),
                BuiltInArgument::Index(3),
                BuiltInArgument::CursorWarp(BuiltInCursorWarpPolicy::FollowConfiguration),
            ])?
        );
        Ok(())
    }

    #[test]
    fn focus_sensitive_cli_actions_follow_configuration() -> Result<(), Box<dyn std::error::Error>>
    {
        assert_eq!(
            focus_stack_window_arguments(2)?,
            built_in_arguments([
                BuiltInArgument::Index(2),
                BuiltInArgument::CursorWarp(BuiltInCursorWarpPolicy::FollowConfiguration),
            ])?
        );
        assert_eq!(
            toggle_workspace_layer_arguments()?,
            built_in_arguments([
                BuiltInArgument::WorkspaceTarget(BuiltInWorkspaceTarget::FocusedAtExecution),
                BuiltInArgument::CursorWarp(BuiltInCursorWarpPolicy::FollowConfiguration),
            ])?
        );
        Ok(())
    }
}
