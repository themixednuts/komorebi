use std::num::NonZeroUsize;

use crate::animation::prefix::AnimationPrefix;
use crate::core::AnimationDuration;
use crate::core::AnimationFps;
use crate::core::AnimationStyle;
use crate::core::ApplicationIdentifier;
use crate::core::Axis;
use crate::core::BorderImplementation;
use crate::core::BorderOffset;
use crate::core::BorderStyle;
use crate::core::BorderWidth;
use crate::core::CycleDirection;
use crate::core::DefaultLayout;
use crate::core::FocusFollowsMouseImplementation;
use crate::core::HidingBehaviour;
use crate::core::MonocleFocusBehaviour;
use crate::core::MoveBehaviour;
use crate::core::OperationBehaviour;
use crate::core::OperationDirection;
use crate::core::ResizeStep;
use crate::core::Sizing;
use crate::core::StackbarFontSize;
use crate::core::StackbarHeight;
use crate::core::StackbarLabel;
use crate::core::StackbarMode;
use crate::core::StackbarTabWidth;
use crate::core::TransparencyAlpha;
use crate::core::WindowKind;
use crate::core::WorkAreaOffset;
use komorebi_themes::colour::Rgb;

use super::builtin::Pixels;
use super::builtin::WorkspaceName;
use super::index::ContainerIndex;
use super::index::MonitorIndex;
use super::index::StackIndex;
use super::index::WorkspaceIndex;
use super::index::WorkspaceLocation;
use super::path::WindowsPath;

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
    ResizeStepSet {
        step: ResizeStep,
    },
    TransparencyEnabledSet {
        enabled: bool,
    },
    TransparencyToggled {
        enabled: bool,
    },
    TransparencyAlphaSet {
        alpha: TransparencyAlpha,
    },
    BorderEnabledSet {
        enabled: bool,
    },
    BorderColourSet {
        window_kind: WindowKind,
        colour: Rgb,
    },
    BorderWidthSet {
        width: BorderWidth,
    },
    BorderOffsetSet {
        offset: BorderOffset,
    },
    BorderStyleSet {
        style: BorderStyle,
    },
    BorderImplementationSet {
        implementation: BorderImplementation,
    },
    StackbarModeSet,
    StackbarLabelSet,
    StackbarFocusedTextColourSet,
    StackbarUnfocusedTextColourSet,
    StackbarBackgroundColourSet,
    StackbarHeightSet,
    StackbarTabWidthSet,
    StackbarFontSizeSet,
    StackbarFontFamilySet,
    AnimationEnabledSet,
    AnimationDurationSet,
    AnimationFpsSet,
    AnimationStyleSet,
    GlobalWorkAreaOffsetSet,
    MonitorWorkAreaOffsetSet,
    WorkspaceWorkAreaOffsetSet,
    WindowBasedWorkAreaOffsetToggled {
        enabled: bool,
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
        index: StackIndex,
    },
    WorkspaceFocused {
        index: WorkspaceIndex,
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
        index: MonitorIndex,
    },
    MonitorCycled {
        direction: CycleDirection,
    },
    MonitorAtCursorFocused,
    WorkspaceFocusedOnAllMonitors {
        index: WorkspaceIndex,
    },
    MonitorWorkspaceFocused {
        monitor: MonitorIndex,
        workspace: WorkspaceIndex,
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
        index: WorkspaceIndex,
    },
    ContainerCycledToWorkspace {
        direction: CycleDirection,
    },
    ContainerSentToWorkspace {
        index: WorkspaceIndex,
    },
    ContainerCycleSentToWorkspace {
        direction: CycleDirection,
    },
    ContainerMovedToMonitor {
        index: MonitorIndex,
    },
    ContainerCycledToMonitor {
        direction: CycleDirection,
    },
    ContainerSentToMonitor {
        index: MonitorIndex,
    },
    ContainerCycleSentToMonitor {
        direction: CycleDirection,
    },
    ContainerMovedToMonitorWorkspace {
        monitor: MonitorIndex,
        workspace: WorkspaceIndex,
    },
    ContainerSentToMonitorWorkspace {
        monitor: MonitorIndex,
        workspace: WorkspaceIndex,
    },
    WorkspaceMovedToMonitor {
        index: MonitorIndex,
    },
    WorkspaceCycledToMonitor {
        direction: CycleDirection,
    },
    WorkspacesSwappedToMonitor {
        index: MonitorIndex,
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
        monitor: MonitorIndex,
        workspace: WorkspaceIndex,
        size: i32,
    },
    WorkspacePaddingSet {
        monitor: MonitorIndex,
        workspace: WorkspaceIndex,
        size: i32,
    },
    WorkspaceTilingSet {
        monitor: MonitorIndex,
        workspace: WorkspaceIndex,
        tile: bool,
    },
    WorkspaceMonocleSet {
        monitor: MonitorIndex,
        workspace: WorkspaceIndex,
        enabled: bool,
    },
    WorkspaceActiveContainerLockSet {
        monitor: MonitorIndex,
        workspace: WorkspaceIndex,
        locked: bool,
    },
    MonitorWorkspaceLayoutSet {
        monitor: MonitorIndex,
        workspace: WorkspaceIndex,
        layout: DefaultLayout,
    },
    WorkspacesEnsured {
        monitor: MonitorIndex,
        count: usize,
    },
    WorkspaceLayoutRulesCleared {
        monitor: MonitorIndex,
        workspace: WorkspaceIndex,
    },
    ScrollingColumnsSet {
        columns: NonZeroUsize,
    },
    ContainerLocked {
        monitor: MonitorIndex,
        workspace: WorkspaceIndex,
        container: ContainerIndex,
    },
    ContainerUnlocked {
        monitor: MonitorIndex,
        workspace: WorkspaceIndex,
        container: ContainerIndex,
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
        monitor: MonitorIndex,
        workspace: WorkspaceIndex,
        at_container_count: usize,
        layout: DefaultLayout,
    },
    LayoutRatiosSet,
    CustomLayoutSet,
    WorkspaceCustomLayoutSet {
        monitor: MonitorIndex,
        workspace: WorkspaceIndex,
    },
    WorkspaceCustomLayoutRuleAdded {
        monitor: MonitorIndex,
        workspace: WorkspaceIndex,
        at_container_count: usize,
    },
    NamedWorkspacesEnsured {
        monitor: MonitorIndex,
    },
    WorkspaceNamed {
        monitor: MonitorIndex,
        workspace: WorkspaceIndex,
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
    SetResizeStep {
        step: ResizeStep,
    },
    SetTransparencyEnabled {
        enabled: bool,
    },
    SetTransparencyAlpha {
        alpha: TransparencyAlpha,
    },
    SetBorderEnabled {
        enabled: bool,
        implementation: BorderImplementation,
    },
    SetBorderColour {
        window_kind: WindowKind,
        colour: Rgb,
    },
    SetBorderWidth {
        width: BorderWidth,
    },
    SetBorderOffset {
        offset: BorderOffset,
    },
    SetBorderStyle {
        style: BorderStyle,
    },
    SetBorderImplementation {
        implementation: BorderImplementation,
    },
    SetStackbarMode {
        mode: StackbarMode,
    },
    SetStackbarLabel {
        label: StackbarLabel,
    },
    SetStackbarFocusedTextColour {
        colour: Rgb,
    },
    SetStackbarUnfocusedTextColour {
        colour: Rgb,
    },
    SetStackbarBackgroundColour {
        colour: Rgb,
    },
    SetStackbarHeight {
        height: StackbarHeight,
    },
    SetStackbarTabWidth {
        width: StackbarTabWidth,
    },
    SetStackbarFontSize {
        size: StackbarFontSize,
    },
    SetStackbarFontFamily {
        family: Option<String>,
    },
    SetAnimationEnabled {
        enabled: bool,
        prefix: Option<AnimationPrefix>,
    },
    SetAnimationDuration {
        duration: AnimationDuration,
        prefix: Option<AnimationPrefix>,
    },
    SetAnimationFps {
        fps: AnimationFps,
    },
    SetAnimationStyle {
        style: AnimationStyle,
        prefix: Option<AnimationPrefix>,
    },
    SetGlobalWorkAreaOffset {
        offset: WorkAreaOffset,
    },
    SetMonitorWorkAreaOffset {
        monitor: MonitorIndex,
        offset: WorkAreaOffset,
    },
    SetWorkspaceWorkAreaOffset {
        location: WorkspaceLocation,
        offset: WorkAreaOffset,
    },
    SetWindowBasedWorkAreaOffset {
        location: WorkspaceLocation,
        enabled: bool,
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
        index: StackIndex,
        cursor_warp: crate::action::CursorWarpPolicy,
    },
    FocusWorkspace {
        index: WorkspaceIndex,
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
        index: MonitorIndex,
    },
    CycleFocusMonitor {
        direction: CycleDirection,
    },
    FocusMonitorAtCursor,
    FocusWorkspaceOnAllMonitors {
        index: WorkspaceIndex,
    },
    FocusMonitorWorkspace {
        monitor: MonitorIndex,
        workspace: WorkspaceIndex,
        cursor_warp: crate::action::CursorWarpPolicy,
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
    ToggleWorkspaceLayer {
        target: crate::action::WorkspaceActionTarget,
        cursor_warp: crate::action::CursorWarpPolicy,
    },
    MoveContainerToLastWorkspace,
    SendContainerToLastWorkspace,
    MoveContainerToWorkspace {
        index: WorkspaceIndex,
    },
    CycleMoveContainerToWorkspace {
        direction: CycleDirection,
    },
    SendContainerToWorkspace {
        index: WorkspaceIndex,
    },
    CycleSendContainerToWorkspace {
        direction: CycleDirection,
    },
    MoveContainerToMonitor {
        index: MonitorIndex,
    },
    CycleMoveContainerToMonitor {
        direction: CycleDirection,
    },
    SendContainerToMonitor {
        index: MonitorIndex,
    },
    CycleSendContainerToMonitor {
        direction: CycleDirection,
    },
    MoveContainerToMonitorWorkspace {
        monitor: MonitorIndex,
        workspace: WorkspaceIndex,
    },
    SendContainerToMonitorWorkspace {
        monitor: MonitorIndex,
        workspace: WorkspaceIndex,
    },
    MoveWorkspaceToMonitor {
        index: MonitorIndex,
    },
    CycleMoveWorkspaceToMonitor {
        direction: CycleDirection,
    },
    SwapWorkspacesToMonitor {
        index: MonitorIndex,
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
        monitor: MonitorIndex,
        workspace: WorkspaceIndex,
        size: i32,
    },
    SetWorkspacePadding {
        monitor: MonitorIndex,
        workspace: WorkspaceIndex,
        size: i32,
    },
    SetWorkspaceTiling {
        monitor: MonitorIndex,
        workspace: WorkspaceIndex,
        tile: bool,
    },
    SetWorkspaceMonocle {
        monitor: MonitorIndex,
        workspace: WorkspaceIndex,
        enabled: bool,
    },
    SetWorkspaceActiveContainerLock {
        monitor: MonitorIndex,
        workspace: WorkspaceIndex,
        locked: bool,
    },
    SetMonitorWorkspaceLayout {
        monitor: MonitorIndex,
        workspace: WorkspaceIndex,
        layout: DefaultLayout,
    },
    EnsureWorkspaces {
        monitor: MonitorIndex,
        count: usize,
    },
    ClearWorkspaceLayoutRules {
        monitor: MonitorIndex,
        workspace: WorkspaceIndex,
    },
    SetScrollingColumns {
        columns: NonZeroUsize,
    },
    LockContainer {
        monitor: MonitorIndex,
        workspace: WorkspaceIndex,
        container: ContainerIndex,
    },
    UnlockContainer {
        monitor: MonitorIndex,
        workspace: WorkspaceIndex,
        container: ContainerIndex,
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
        monitor: MonitorIndex,
        workspace: WorkspaceIndex,
        at_container_count: usize,
        layout: DefaultLayout,
    },
    SetLayoutRatios {
        columns: Option<Vec<f32>>,
        rows: Option<Vec<f32>>,
    },
    SetCustomLayout {
        path: WindowsPath,
    },
    SetWorkspaceCustomLayout {
        monitor: MonitorIndex,
        workspace: WorkspaceIndex,
        path: WindowsPath,
    },
    AddWorkspaceCustomLayoutRule {
        monitor: MonitorIndex,
        workspace: WorkspaceIndex,
        at_container_count: usize,
        path: WindowsPath,
    },
    EnsureNamedWorkspaces {
        monitor: MonitorIndex,
        names: Vec<WorkspaceName>,
    },
    SetWorkspaceName {
        monitor: MonitorIndex,
        workspace: WorkspaceIndex,
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
