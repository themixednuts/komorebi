use super::WindowManager;
use crate::container::Container;
use crate::core::Rect;
use crate::monitor;
use crate::monitor::Monitor;
use crate::workspace::WorkspaceLayer;
use komorebi_protocol::ManagerEpoch;

fn manager() -> WindowManager {
    WindowManager::new(ManagerEpoch::new([1; 16]).expect("test epoch is non-nil"))
        .expect("test manager should initialize")
}

fn monitor() -> Monitor {
    monitor::new(
        0,
        Rect::default(),
        Rect::default(),
        "TestMonitor".to_string(),
        "TestDevice".to_string(),
        "TestDeviceID".to_string(),
        Some("TestMonitorID".to_string()),
    )
}

#[test]
fn active_tiled_container_lock_is_exact_and_idempotent() {
    let mut manager = manager();
    let mut monitor = monitor();
    monitor
        .focused_workspace_mut()
        .expect("test workspace should exist")
        .add_container_to_back(Container::default());
    manager.monitors_mut().push_back(monitor);

    manager
        .set_workspace_active_container_locked(0, 0, true)
        .expect("active tiled container should lock");
    manager
        .set_workspace_active_container_locked(0, 0, true)
        .expect("setting the same lock state should be idempotent");
    assert!(focused_tiled_container(&manager).locked);

    manager
        .set_workspace_active_container_locked(0, 0, false)
        .expect("active tiled container should unlock");
    assert!(!focused_tiled_container(&manager).locked);
}

#[test]
fn active_monocle_container_is_lockable() {
    let mut manager = manager();
    let mut monitor = monitor();
    monitor
        .focused_workspace_mut()
        .expect("test workspace should exist")
        .monocle_container = Some(Container::default());
    manager.monitors_mut().push_back(monitor);

    manager
        .set_workspace_active_container_locked(0, 0, true)
        .expect("active monocle container should lock");

    assert!(
        manager
            .focused_workspace()
            .expect("test workspace should exist")
            .monocle_container
            .as_ref()
            .expect("test monocle should exist")
            .locked
    );
}

#[test]
fn non_lockable_target_rejects_before_focus_changes() {
    let mut manager = manager();
    let mut monitor = monitor();
    monitor
        .focused_workspace_mut()
        .expect("test workspace should exist")
        .layer = WorkspaceLayer::Floating;
    let focused_workspace = monitor.new_workspace_idx();
    monitor
        .focus_workspace(focused_workspace)
        .expect("second test workspace should be created");
    manager.monitors_mut().push_back(monitor);

    let result = manager.set_workspace_active_container_locked(0, 0, true);

    assert!(result.is_err());
    assert_eq!(
        manager
            .focused_workspace_idx()
            .expect("focused workspace should still exist"),
        focused_workspace
    );
}

fn focused_tiled_container(manager: &WindowManager) -> &Container {
    manager
        .focused_workspace()
        .expect("test workspace should exist")
        .focused_container()
        .expect("test container should exist")
}
