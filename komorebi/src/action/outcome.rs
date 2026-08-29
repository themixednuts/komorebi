use std::num::NonZeroUsize;
use std::path::PathBuf;

use crate::core::ApplicationIdentifier;
use crate::core::Axis;
use crate::core::CycleDirection;
use crate::core::DefaultLayout;
use crate::core::FocusFollowsMouseImplementation;
use crate::core::HidingBehaviour;
use crate::core::MonocleFocusBehaviour;
use crate::core::MoveBehaviour;
use crate::core::OperationBehaviour;
use crate::core::OperationDirection;
use crate::core::Sizing;

use super::builtin::Pixels;
use super::builtin::WorkspaceName;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActionResult {
    Focused {
        direction: OperationDirection,
    },
    Moved {
        direction: OperationDirection,
    },
    Resized {
        axis: Axis,
        delta: Pixels,
    },
    LayoutSet {
        layout: DefaultLayout,
    },
    FloatToggled {
        floating: bool,
    },
    CycleFocused {
        direction: CycleDirection,
    },
    CycleMoved {
        direction: CycleDirection,
    },
    MonocleToggled,
    MaximizeToggled,
    LockToggled,
    Stacked {
        direction: OperationDirection,
    },
    Unstacked,
    StackedAll,
    UnstackedAll,
    StackCycled {
        direction: CycleDirection,
    },
    StackIndexCycled {
        direction: CycleDirection,
    },
    StackWindowFocused {
        index: usize,
    },
    WorkspaceFocused {
        index: usize,
    },
    WorkspaceCycled {
        direction: CycleDirection,
    },
    EmptyWorkspaceCycled {
        direction: CycleDirection,
    },
    LastWorkspaceFocused,
    WorkspaceClosed,
    MonitorFocused {
        index: usize,
    },
    MonitorCycled {
        direction: CycleDirection,
    },
    MonitorAtCursorFocused,
    WorkspaceFocusedOnAllMonitors {
        index: usize,
    },
    MonitorWorkspaceFocused {
        monitor: usize,
        workspace: usize,
    },
    WindowClosed,
    WindowMinimized,
    FocusForced,
    ContainerPromoted,
    ContainerPromoteSwapped,
    FocusPromoted,
    WindowPromoted {
        direction: OperationDirection,
    },
    WorkspaceCreated,
    TilingToggled,
    LayoutCycled {
        direction: CycleDirection,
    },
    LayoutFlipped {
        axis: Axis,
    },
    WorkspaceLayerToggled,
    ContainerMovedToLastWorkspace,
    ContainerSentToLastWorkspace,
    ContainerMovedToWorkspace {
        index: usize,
    },
    ContainerCycledToWorkspace {
        direction: CycleDirection,
    },
    ContainerSentToWorkspace {
        index: usize,
    },
    ContainerCycleSentToWorkspace {
        direction: CycleDirection,
    },
    ContainerMovedToMonitor {
        index: usize,
    },
    ContainerCycledToMonitor {
        direction: CycleDirection,
    },
    ContainerSentToMonitor {
        index: usize,
    },
    ContainerCycleSentToMonitor {
        direction: CycleDirection,
    },
    ContainerMovedToMonitorWorkspace {
        monitor: usize,
        workspace: usize,
    },
    ContainerSentToMonitorWorkspace {
        monitor: usize,
        workspace: usize,
    },
    WorkspaceMovedToMonitor {
        index: usize,
    },
    WorkspaceCycledToMonitor {
        direction: CycleDirection,
    },
    WorkspacesSwappedToMonitor {
        index: usize,
    },
    DirectionPreselected {
        direction: OperationDirection,
    },
    PreselectCancelled,
    Retiled,
    RetiledWithResizeDimensions,
    FocusedWindowManaged,
    FocusedWindowUnmanaged,
    ContainerPaddingAdjusted {
        sizing: Sizing,
        adjustment: i32,
    },
    WorkspacePaddingAdjusted {
        sizing: Sizing,
        adjustment: i32,
    },
    MouseFollowsFocusToggled,
    MouseFollowsFocusSet {
        enabled: bool,
    },
    WindowContainerBehaviourToggled,
    FloatOverrideToggled,
    WorkspaceWindowContainerBehaviourToggled,
    WorkspaceFloatOverrideToggled,
    CrossMonitorMoveBehaviourToggled,
    MonocleFocusBehaviourToggled,
    PauseToggled {
        paused: bool,
    },
    FocusedContainerPaddingSet {
        size: i32,
    },
    FocusedWorkspacePaddingSet {
        size: i32,
    },
    ContainerPaddingSet {
        monitor: usize,
        workspace: usize,
        size: i32,
    },
    WorkspacePaddingSet {
        monitor: usize,
        workspace: usize,
        size: i32,
    },
    WorkspaceTilingSet {
        monitor: usize,
        workspace: usize,
        tile: bool,
    },
    MonitorWorkspaceLayoutSet {
        monitor: usize,
        workspace: usize,
        layout: DefaultLayout,
    },
    WorkspacesEnsured {
        monitor: usize,
        count: usize,
    },
    WorkspaceLayoutRulesCleared {
        monitor: usize,
        workspace: usize,
    },
    ScrollingColumnsSet {
        columns: NonZeroUsize,
    },
    ContainerLocked {
        monitor: usize,
        workspace: usize,
        container: usize,
    },
    ContainerUnlocked {
        monitor: usize,
        workspace: usize,
        container: usize,
    },
    TitleBarsToggled,
    WorkspaceRulesEnforced,
    SessionFloatRuleAdded,
    SessionFloatRulesCleared,
    WindowEdgeResized {
        direction: OperationDirection,
        delta: Pixels,
    },
    WindowHidingBehaviourSet {
        behaviour: HidingBehaviour,
    },
    CrossMonitorMoveBehaviourSet {
        behaviour: MoveBehaviour,
    },
    MonocleFocusBehaviourSet {
        behaviour: MonocleFocusBehaviour,
    },
    UnmanagedWindowOperationBehaviourSet {
        behaviour: OperationBehaviour,
    },
    FocusFollowsMouseSet {
        implementation: FocusFollowsMouseImplementation,
        enabled: bool,
    },
    FocusFollowsMouseToggled {
        implementation: FocusFollowsMouseImplementation,
    },
    WorkspaceLayoutRuleAdded {
        monitor: usize,
        workspace: usize,
        at_container_count: usize,
        layout: DefaultLayout,
    },
    LayoutRatiosSet,
    CustomLayoutSet,
    WorkspaceCustomLayoutSet {
        monitor: usize,
        workspace: usize,
    },
    WorkspaceCustomLayoutRuleAdded {
        monitor: usize,
        workspace: usize,
        at_container_count: usize,
    },
    NamedWorkspacesEnsured {
        monitor: usize,
    },
    WorkspaceNamed {
        monitor: usize,
        workspace: usize,
    },
    EagerFocused,
    TitleBarRemoved,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct EffectId(u64);

impl EffectId {
    #[must_use]
    pub const fn new(ordinal: u64) -> Self {
        Self(ordinal)
    }

    #[must_use]
    pub const fn ordinal(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlannedEffect {
    pub id: EffectId,
    pub effect: NativeEffect,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeEffectFailure {
    pub effect_id: EffectId,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum NativeEffect {
    FocusNeighbor {
        direction: OperationDirection,
    },
    MoveNeighbor {
        direction: OperationDirection,
    },
    Resize {
        axis: Axis,
        delta: Pixels,
    },
    SetLayout {
        layout: DefaultLayout,
    },
    SetWindowFloating {
        floating: bool,
    },
    CycleFocus {
        direction: CycleDirection,
    },
    CycleMove {
        direction: CycleDirection,
    },
    ToggleMonocle,
    ToggleMaximize,
    ToggleLock,
    Stack {
        direction: OperationDirection,
    },
    Unstack,
    StackAll,
    UnstackAll,
    CycleStack {
        direction: CycleDirection,
    },
    CycleStackIndex {
        direction: CycleDirection,
    },
    FocusStack {
        index: usize,
    },
    FocusWorkspace {
        index: usize,
    },
    CycleFocusWorkspace {
        direction: CycleDirection,
    },
    CycleFocusEmptyWorkspace {
        direction: CycleDirection,
    },
    FocusLastWorkspace,
    CloseWorkspace,
    FocusMonitor {
        index: usize,
    },
    CycleFocusMonitor {
        direction: CycleDirection,
    },
    FocusMonitorAtCursor,
    FocusWorkspaceOnAllMonitors {
        index: usize,
    },
    FocusMonitorWorkspace {
        monitor: usize,
        workspace: usize,
    },
    CloseWindow,
    MinimizeWindow,
    ForceFocus,
    PromoteContainer,
    PromoteContainerSwap,
    PromoteFocus,
    PromoteWindow {
        direction: OperationDirection,
    },
    CreateWorkspace,
    ToggleTiling,
    CycleLayout {
        direction: CycleDirection,
    },
    FlipLayout {
        axis: Axis,
    },
    ToggleWorkspaceLayer,
    MoveContainerToLastWorkspace,
    SendContainerToLastWorkspace,
    MoveContainerToWorkspace {
        index: usize,
    },
    CycleMoveContainerToWorkspace {
        direction: CycleDirection,
    },
    SendContainerToWorkspace {
        index: usize,
    },
    CycleSendContainerToWorkspace {
        direction: CycleDirection,
    },
    MoveContainerToMonitor {
        index: usize,
    },
    CycleMoveContainerToMonitor {
        direction: CycleDirection,
    },
    SendContainerToMonitor {
        index: usize,
    },
    CycleSendContainerToMonitor {
        direction: CycleDirection,
    },
    MoveContainerToMonitorWorkspace {
        monitor: usize,
        workspace: usize,
    },
    SendContainerToMonitorWorkspace {
        monitor: usize,
        workspace: usize,
    },
    MoveWorkspaceToMonitor {
        index: usize,
    },
    CycleMoveWorkspaceToMonitor {
        direction: CycleDirection,
    },
    SwapWorkspacesToMonitor {
        index: usize,
    },
    PreselectDirection {
        direction: OperationDirection,
    },
    CancelPreselect,
    Retile,
    RetileWithResizeDimensions,
    ManageFocusedWindow,
    UnmanageFocusedWindow,
    AdjustContainerPadding {
        sizing: Sizing,
        adjustment: i32,
    },
    AdjustWorkspacePadding {
        sizing: Sizing,
        adjustment: i32,
    },
    ToggleMouseFollowsFocus,
    SetMouseFollowsFocus {
        enabled: bool,
    },
    ToggleWindowContainerBehaviour,
    ToggleFloatOverride,
    ToggleWorkspaceWindowContainerBehaviour,
    ToggleWorkspaceFloatOverride,
    ToggleCrossMonitorMoveBehaviour,
    ToggleMonocleFocusBehaviour,
    TogglePause,
    SetFocusedContainerPadding {
        size: i32,
    },
    SetFocusedWorkspacePadding {
        size: i32,
    },
    SetContainerPadding {
        monitor: usize,
        workspace: usize,
        size: i32,
    },
    SetWorkspacePadding {
        monitor: usize,
        workspace: usize,
        size: i32,
    },
    SetWorkspaceTiling {
        monitor: usize,
        workspace: usize,
        tile: bool,
    },
    SetMonitorWorkspaceLayout {
        monitor: usize,
        workspace: usize,
        layout: DefaultLayout,
    },
    EnsureWorkspaces {
        monitor: usize,
        count: usize,
    },
    ClearWorkspaceLayoutRules {
        monitor: usize,
        workspace: usize,
    },
    SetScrollingColumns {
        columns: NonZeroUsize,
    },
    LockContainer {
        monitor: usize,
        workspace: usize,
        container: usize,
    },
    UnlockContainer {
        monitor: usize,
        workspace: usize,
        container: usize,
    },
    ToggleTitleBars,
    EnforceWorkspaceRules,
    AddSessionFloatRule,
    ClearSessionFloatRules,
    ResizeEdge {
        direction: OperationDirection,
        delta: Pixels,
    },
    SetWindowHidingBehaviour {
        behaviour: HidingBehaviour,
    },
    SetCrossMonitorMoveBehaviour {
        behaviour: MoveBehaviour,
    },
    SetMonocleFocusBehaviour {
        behaviour: MonocleFocusBehaviour,
    },
    SetUnmanagedWindowOperationBehaviour {
        behaviour: OperationBehaviour,
    },
    SetFocusFollowsMouse {
        implementation: FocusFollowsMouseImplementation,
        enabled: bool,
    },
    ToggleFocusFollowsMouse {
        implementation: FocusFollowsMouseImplementation,
    },
    AddWorkspaceLayoutRule {
        monitor: usize,
        workspace: usize,
        at_container_count: usize,
        layout: DefaultLayout,
    },
    SetLayoutRatios {
        columns: Option<Vec<f32>>,
        rows: Option<Vec<f32>>,
    },
    SetCustomLayout {
        path: PathBuf,
    },
    SetWorkspaceCustomLayout {
        monitor: usize,
        workspace: usize,
        path: PathBuf,
    },
    AddWorkspaceCustomLayoutRule {
        monitor: usize,
        workspace: usize,
        at_container_count: usize,
        path: PathBuf,
    },
    EnsureNamedWorkspaces {
        monitor: usize,
        names: Vec<WorkspaceName>,
    },
    SetWorkspaceName {
        monitor: usize,
        workspace: usize,
        name: WorkspaceName,
    },
    EagerFocus {
        exe: String,
    },
    RemoveTitleBar {
        identifier: ApplicationIdentifier,
        id: String,
    },
}
