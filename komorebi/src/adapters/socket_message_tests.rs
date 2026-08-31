use super::*;
use crate::core::DefaultLayout;
use crate::core::OperationDirection;

fn stamp(revision: u64) -> komorebi_protocol::StateStamp {
    komorebi_protocol::StateStamp::new(
        komorebi_protocol::ManagerEpoch::new([1; 16]).expect("test epoch is non-nil"),
        komorebi_protocol::Revision::try_from(revision).expect("test revision is nonzero"),
    )
}

#[test]
fn migrated_socket_messages_become_the_same_builtin_action()
-> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(
        to_builtin_action(&SocketMessage::FocusWindow(OperationDirection::Left)),
        Some(BuiltinAction::FocusWindow {
            direction: OperationDirection::Left,
        })
    );
    assert_eq!(
        classify(&SocketMessage::FocusWindow(OperationDirection::Left)),
        SocketMessageClass::Action
    );
    assert_eq!(
        to_builtin_action(&SocketMessage::MoveWindow(OperationDirection::Right)),
        Some(BuiltinAction::MoveWindow {
            direction: OperationDirection::Right,
        })
    );
    assert_eq!(
        to_builtin_action(&SocketMessage::ChangeLayout(DefaultLayout::Columns)),
        Some(BuiltinAction::SetWorkspaceLayout {
            workspace: WorkspaceSelector::FocusedAtExecution,
            layout: DefaultLayout::Columns,
        })
    );
    assert_eq!(
        to_builtin_action(&SocketMessage::ToggleFloat),
        Some(BuiltinAction::ToggleWindowFloat {
            window: WindowSelector::FocusedAtExecution,
        })
    );
    assert_eq!(
        to_builtin_action(&SocketMessage::ResizeWindowAxis(
            crate::core::Axis::Horizontal,
            crate::core::Sizing::Decrease
        )),
        Some(BuiltinAction::ResizeWindowByStep {
            axis: crate::core::Axis::Horizontal,
            sizing: crate::core::Sizing::Decrease,
        })
    );
    assert_eq!(
        to_builtin_action(&SocketMessage::ResizeDelta(37)),
        Some(BuiltinAction::SetResizeStep {
            step: ResizeStep::new(37)?,
        })
    );
    assert_eq!(
        to_builtin_action(&SocketMessage::Transparency(true)),
        Some(BuiltinAction::SetTransparencyEnabled { enabled: true })
    );
    assert_eq!(
        to_builtin_action(&SocketMessage::ToggleTransparency),
        Some(BuiltinAction::ToggleTransparency)
    );
    assert_eq!(
        to_builtin_action(&SocketMessage::TransparencyAlpha(177)),
        Some(BuiltinAction::SetTransparencyAlpha {
            alpha: TransparencyAlpha::new(177),
        })
    );
    assert_eq!(
        to_builtin_action(&SocketMessage::CycleFocusWindow(
            crate::core::CycleDirection::Next
        )),
        Some(BuiltinAction::CycleFocusWindow {
            direction: crate::core::CycleDirection::Next,
        })
    );
    assert_eq!(
        to_builtin_action(&SocketMessage::ToggleMonocle),
        Some(BuiltinAction::ToggleWindowMonocle {
            window: WindowSelector::FocusedAtExecution,
        })
    );
    assert_eq!(
        to_builtin_action(&SocketMessage::CycleFocusWorkspace(
            crate::core::CycleDirection::Next
        )),
        Some(BuiltinAction::CycleFocusWorkspace {
            direction: crate::core::CycleDirection::Next,
        })
    );
    assert_eq!(
        to_builtin_action(&SocketMessage::FocusMonitorNumber(2)),
        Some(BuiltinAction::FocusMonitor {
            index: MonitorIndex::new(2),
        })
    );
    assert_eq!(
        to_builtin_action(&SocketMessage::FocusWorkspaceNumbers(3)),
        Some(BuiltinAction::FocusWorkspaceOnAllMonitors {
            index: WorkspaceIndex::new(3),
        })
    );
    assert_eq!(classify(&SocketMessage::State), SocketMessageClass::Query);
    assert_eq!(
        classify(&SocketMessage::AddSubscriberSocket(
            crate::core::SubscriberName::parse("komorebi-bar-forest").unwrap()
        )),
        SocketMessageClass::Subscription
    );
    assert_eq!(
        classify(&SocketMessage::Stop),
        SocketMessageClass::SchemaDebugAdmin
    );
    assert_eq!(
        to_builtin_action(&SocketMessage::ResizeWindowEdge(
            OperationDirection::Right,
            crate::core::Sizing::Increase
        )),
        Some(BuiltinAction::ResizeWindowEdgeByStep {
            direction: OperationDirection::Right,
            sizing: crate::core::Sizing::Increase,
        })
    );
    assert_eq!(
        to_builtin_action(&SocketMessage::FocusNamedWorkspace("chat".into())),
        Some(BuiltinAction::FocusNamedWorkspace {
            name: crate::action::WorkspaceName::parse("chat").unwrap(),
        })
    );
    assert_eq!(
        to_builtin_action(&SocketMessage::FocusNamedWorkspace("".into())),
        None
    );
    assert_eq!(
        to_builtin_action(&SocketMessage::CrossMonitorMoveBehaviour(
            crate::core::MoveBehaviour::Insert
        )),
        Some(BuiltinAction::SetCrossMonitorMoveBehaviour {
            behaviour: crate::core::MoveBehaviour::Insert,
        })
    );
    Ok(())
}

#[test]
fn socket_focus_admits_as_the_same_catalog_action() {
    use crate::action::ActionGrants;
    use crate::action::ActionSnapshot;
    use crate::action::CatalogState;
    use crate::action::InvocationContext;
    use crate::action::InvocationId;
    use komorebi_protocol::InvocationNamespaceId;
    use komorebi_protocol::InvocationSequence;

    fn invocation() -> InvocationId {
        InvocationId::new(
            InvocationNamespaceId::new([9; 16]).expect("test namespace is nonzero"),
            InvocationSequence::try_from(1).expect("test sequence is nonzero"),
        )
    }

    fn principal() -> PrincipalId {
        PrincipalId::new([1; 32]).expect("test principal is nonzero")
    }
    use crate::action::InvocationOrigin;
    use crate::action::InvokeAction;
    use crate::action::PrincipalId;
    use crate::action::id::WindowId;
    use crate::action::invoke::ActionAdmission;
    use std::time::Instant;

    let message = SocketMessage::FocusWindow(OperationDirection::Left);
    let action = to_builtin_action(&message).expect("focus-window is migrated");
    let mut state = CatalogState::new(ActionSnapshot {
        state: stamp(1),
        paused: false,
        focused_window: Some(WindowId::new(9)),
        focused_workspace: Some(crate::action::WorkspaceLocation::new(
            crate::action::MonitorIndex::new(0),
            crate::action::WorkspaceIndex::new(0),
        )),
        directional_targets: [OperationDirection::Left].into(),
        current_layout: DefaultLayout::BSP,
        configuration: crate::action::ConfigurationSnapshot::default(),
        focused_window_floating: false,
        named_workspaces: Vec::new(),
        bindings: Vec::new(),
    });
    let admission = state.admit(
        InvokeAction {
            invocation_id: invocation(),
            expected_state: stamp(1),
            action,
            confirmation: None,
        },
        &InvocationContext {
            principal: principal(),
            origin: InvocationOrigin::Cli,
            grants: ActionGrants::all(),
        },
        Instant::now(),
    );
    assert!(matches!(
        admission,
        ActionAdmission::Committed { state, .. } if state == stamp(2)
    ));
}

#[test]
fn adapter_separates_non_actions_from_invalid_actions() {
    assert_eq!(adapt_action(&SocketMessage::State), Ok(None));
    assert_eq!(
        adapt_action(&SocketMessage::FocusNamedWorkspace(String::new())),
        Err(SocketActionAdapterError::InvalidParameters)
    );
    assert_eq!(
        adapt_action(&SocketMessage::ResizeDelta(0)),
        Err(SocketActionAdapterError::InvalidParameters)
    );
    assert_eq!(
        adapt_action(&SocketMessage::ResizeDelta(-1)),
        Err(SocketActionAdapterError::InvalidParameters)
    );
}

#[cfg(windows)]
#[test]
fn adapter_rejects_a_custom_layout_path_with_an_interior_nul() {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use std::path::PathBuf;

    let path = PathBuf::from(OsString::from_wide(&[b'C' as u16, 0, b'x' as u16]));
    assert_eq!(
        adapt_action(&SocketMessage::ChangeLayoutCustom(path)),
        Err(SocketActionAdapterError::InvalidParameters)
    );
}
