use color_eyre::eyre;

use crate::CrossBoundaryBehaviour;
use crate::HIDING_BEHAVIOUR;
use crate::WINDOWS_11;
use crate::action::ActionAdmission;
use crate::action::ActionGrants;
use crate::action::ActionRejection;
use crate::action::ActionSnapshot;
use crate::action::AnimationConfiguration;
use crate::action::AnimationStyleSnapshot;
use crate::action::BorderConfiguration;
use crate::action::BuiltinAction;
use crate::action::ConfigurationSnapshot;
use crate::action::DirectionSet;
use crate::action::InvocationContext;
use crate::action::InvocationId;
use crate::action::InvocationOrigin;
use crate::action::InvokeAction;
use crate::action::MonitorIndex;
use crate::action::NamedWorkspaceTarget;
use crate::action::NativeEffect;
use crate::action::NativeEffectFailure;
use crate::action::ObservationChange;
use crate::action::PlannedEffect;
use crate::action::PrincipalId;
use crate::action::ScopedAnimationValue;
use crate::action::StackbarConfiguration;
use crate::action::TransparencyConfiguration;
use crate::action::WorkspaceIndex;
use crate::action::WorkspaceName;
use crate::action::id::WindowId;
use crate::adapters::action_catalog::CatalogProjectionError;
use crate::adapters::action_catalog::reply as project_catalog_reply;
use crate::animation::ANIMATION_DURATION_GLOBAL;
use crate::animation::ANIMATION_DURATION_PER_ANIMATION;
use crate::animation::ANIMATION_ENABLED_GLOBAL;
use crate::animation::ANIMATION_ENABLED_PER_ANIMATION;
use crate::animation::ANIMATION_STYLE_GLOBAL;
use crate::animation::ANIMATION_STYLE_PER_ANIMATION;
use crate::animation::animation_fps;
use crate::animation::prefix::AnimationPrefix;
use crate::animation::set_animation_fps;
use crate::border_manager;
use crate::core::AnimationDuration;
use crate::core::BorderImplementation;
use crate::core::BorderOffset;
use crate::core::BorderWidth;
use crate::core::DefaultLayout;
use crate::core::Layout;
use crate::core::OperationDirection;
use crate::core::Sizing;
use crate::core::StackbarFontSize;
use crate::core::StackbarHeight;
use crate::core::StackbarTabWidth;
use crate::core::TransparencyAlpha;
use crate::core::WindowKind;
use crate::stackbar_manager;
use crate::transparency_manager;
use crate::workspace::WorkspaceLayer;
use komorebi_protocol::CatalogReply;
use komorebi_protocol::CatalogStamp;
use komorebi_protocol::InvocationIdentityError;
use komorebi_protocol::SettledInvocationKind;
use std::sync::atomic::Ordering;

use super::WindowManager;

#[derive(Debug, thiserror::Error)]
pub enum CatalogActionError {
    #[error("could not issue a local invocation identity: {0}")]
    Identity(#[from] InvocationIdentityError),
    #[error("manager observation failed for invocation {invocation_id:?}: {source}")]
    Observation {
        invocation_id: InvocationId,
        #[source]
        source: komorebi_protocol::ActionContractError,
    },
    #[error("invocation {invocation_id:?} was rejected: {source}")]
    Rejected {
        invocation_id: InvocationId,
        #[source]
        source: ActionRejection,
    },
    #[error("native effects failed for invocation {invocation_id:?}")]
    NativeEffects {
        invocation_id: InvocationId,
        failure: NativeEffectFailure,
        #[source]
        source: eyre::Report,
    },
}

#[derive(Debug, thiserror::Error)]
#[error("native effect {failure:?} failed")]
struct NativeEffectExecutionError {
    failure: NativeEffectFailure,
    #[source]
    source: eyre::Report,
}

impl CatalogActionError {
    #[must_use]
    pub const fn invocation_id(&self) -> Option<InvocationId> {
        match self {
            Self::Identity(_) => None,
            Self::Observation { invocation_id, .. }
            | Self::Rejected { invocation_id, .. }
            | Self::NativeEffects { invocation_id, .. } => Some(*invocation_id),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CatalogReplyError {
    #[error("manager observation failed: {0}")]
    Observation(#[from] komorebi_protocol::ActionContractError),
    #[error(transparent)]
    Projection(#[from] CatalogProjectionError),
}

impl WindowManager {
    /// Reconciles the manager's current semantic state with the action catalog.
    ///
    /// # Errors
    ///
    /// Returns an error without changing the catalog if its revision is
    /// exhausted and the observation changed.
    pub fn refresh_catalog_observation(
        &mut self,
    ) -> Result<ObservationChange, komorebi_protocol::ActionContractError> {
        let snapshot = self.observe_action_snapshot();
        self.catalog.reconcile_observation(snapshot)
    }

    /// Observes current action state and returns an authority-scoped wire
    /// catalog with exact cache semantics.
    ///
    /// # Errors
    ///
    /// Returns [`CatalogReplyError`] if observation revisioning is exhausted or
    /// the internal catalog violates a bounded public protocol invariant.
    pub fn action_catalog_reply(
        &mut self,
        grants: ActionGrants,
        known: Option<CatalogStamp>,
    ) -> Result<CatalogReply, CatalogReplyError> {
        self.refresh_catalog_observation()?;
        Ok(project_catalog_reply(
            self.catalog.snapshot(),
            &crate::action::ActionAuthority { grants },
            known,
        )?)
    }

    #[must_use]
    pub fn observe_action_snapshot(&self) -> ActionSnapshot {
        let focused_window = self
            .focused_window()
            .ok()
            .map(|window| WindowId::new(window.hwnd as u64));
        let current_layout = self
            .focused_workspace()
            .ok()
            .and_then(|workspace| match workspace.layout {
                Layout::Default(layout) => Some(layout),
                Layout::Custom(_) => None,
            })
            .unwrap_or(DefaultLayout::BSP);
        let focused_window_floating = self
            .focused_workspace()
            .ok()
            .is_some_and(|workspace| workspace.layer == WorkspaceLayer::Floating);
        let animation = observe_animation_configuration();
        ActionSnapshot {
            state: self.catalog.snapshot().state,
            paused: self.is_paused,
            focused_window,
            directional_targets: [
                OperationDirection::Left,
                OperationDirection::Right,
                OperationDirection::Up,
                OperationDirection::Down,
            ]
            .into_iter()
            .filter(|direction| self.can_focus_in_direction(*direction))
            .collect::<DirectionSet>(),
            current_layout,
            configuration: ConfigurationSnapshot {
                resize_step: self.resize_step,
                border: BorderConfiguration {
                    enabled: border_manager::BORDER_ENABLED.load(Ordering::SeqCst),
                    width: BorderWidth::new(border_manager::BORDER_WIDTH.load(Ordering::SeqCst)),
                    offset: BorderOffset::new(border_manager::BORDER_OFFSET.load(Ordering::SeqCst)),
                    style: border_manager::STYLE.load(),
                    implementation: border_manager::IMPLEMENTATION.load(),
                },
                transparency: TransparencyConfiguration {
                    enabled: transparency_manager::TRANSPARENCY_ENABLED.load(Ordering::SeqCst),
                    alpha: TransparencyAlpha::new(
                        transparency_manager::TRANSPARENCY_ALPHA.load(Ordering::SeqCst),
                    ),
                },
                stackbar: StackbarConfiguration {
                    mode: stackbar_manager::STACKBAR_MODE.load(),
                    label: stackbar_manager::STACKBAR_LABEL.load(),
                    focused_text_colour: stackbar_manager::STACKBAR_FOCUSED_TEXT_COLOUR
                        .load(Ordering::SeqCst)
                        .into(),
                    unfocused_text_colour: stackbar_manager::STACKBAR_UNFOCUSED_TEXT_COLOUR
                        .load(Ordering::SeqCst)
                        .into(),
                    background_colour: stackbar_manager::STACKBAR_TAB_BACKGROUND_COLOUR
                        .load(Ordering::SeqCst)
                        .into(),
                    height: StackbarHeight::new(
                        stackbar_manager::STACKBAR_TAB_HEIGHT.load(Ordering::SeqCst),
                    ),
                    tab_width: StackbarTabWidth::new(
                        stackbar_manager::STACKBAR_TAB_WIDTH.load(Ordering::SeqCst),
                    ),
                    font_size: StackbarFontSize::new(
                        stackbar_manager::STACKBAR_FONT_SIZE.load(Ordering::SeqCst),
                    ),
                    font_family: stackbar_manager::STACKBAR_FONT_FAMILY
                        .lock()
                        .clone()
                        .map(String::into_boxed_str),
                },
                animation: std::sync::Arc::new(animation),
            },
            focused_window_floating,
            named_workspaces: self.named_workspaces_for_catalog(),
            bindings: Vec::new(),
        }
    }

    fn named_workspaces_for_catalog(&self) -> Vec<NamedWorkspaceTarget> {
        let mut named = Vec::new();
        for (monitor_idx, monitor) in self.monitors().iter().enumerate() {
            for (workspace_idx, workspace) in monitor.workspaces().iter().enumerate() {
                if let Some(name) = &workspace.name
                    && let Ok(name) = crate::action::WorkspaceName::parse(name)
                {
                    named.push(NamedWorkspaceTarget {
                        name,
                        monitor: MonitorIndex::new(monitor_idx),
                        workspace: WorkspaceIndex::new(workspace_idx),
                    });
                }
            }
        }
        named
    }

    fn can_focus_in_direction(&self, direction: OperationDirection) -> bool {
        let Ok(workspace) = self.focused_workspace() else {
            return false;
        };
        if workspace.new_idx_for_direction(direction).is_some() {
            return true;
        }
        if matches!(
            self.cross_boundary_behaviour,
            CrossBoundaryBehaviour::Workspace
        ) && matches!(
            direction,
            OperationDirection::Left | OperationDirection::Right
        ) && self
            .focused_monitor()
            .is_some_and(|monitor| monitor.workspaces().len() > 1)
        {
            return true;
        }
        self.monitor_idx_in_direction(direction).is_some()
    }

    pub fn invoke_catalog_action(
        &mut self,
        request: InvokeAction,
        context: &InvocationContext,
    ) -> Result<InvocationId, CatalogActionError> {
        let invocation_id = request.invocation_id;
        self.refresh_catalog_observation()
            .map_err(|source| CatalogActionError::Observation {
                invocation_id,
                source,
            })?;
        let admission = self
            .catalog
            .admit(request, context, std::time::Instant::now());
        match admission {
            ActionAdmission::Rejected(source) => Err(CatalogActionError::Rejected {
                invocation_id,
                source,
            }),
            ActionAdmission::Committed {
                logical_result,
                effects,
                ..
            } => match self.apply_catalog_effects(&effects) {
                Ok(()) => {
                    self.catalog.settle(invocation_id, logical_result);
                    Ok(invocation_id)
                }
                Err(error) => {
                    self.catalog
                        .degrade(invocation_id, vec![error.failure.clone()]);
                    Err(CatalogActionError::NativeEffects {
                        invocation_id,
                        failure: error.failure,
                        source: error.source,
                    })
                }
            },
        }
    }

    pub(crate) fn admit_socket_action(
        &mut self,
        action: BuiltinAction,
    ) -> Result<InvocationId, CatalogActionError> {
        let invocation_id = self.catalog.issue_local_invocation_id()?;
        self.refresh_catalog_observation()
            .map_err(|source| CatalogActionError::Observation {
                invocation_id,
                source,
            })?;
        let request = InvokeAction {
            invocation_id,
            expected_state: self.catalog.snapshot().state,
            action,
            confirmation: None,
        };
        self.invoke_catalog_action(
            request,
            &InvocationContext {
                principal: PrincipalId::new([u8::MAX; 32])?,
                origin: InvocationOrigin::Ipc,
                grants: ActionGrants::all(),
            },
        )
    }

    fn apply_catalog_effects(
        &mut self,
        effects: &[PlannedEffect],
    ) -> Result<(), NativeEffectExecutionError> {
        for planned in effects {
            if let Err(source) = self.apply_catalog_effect(&planned.effect) {
                return Err(NativeEffectExecutionError {
                    failure: NativeEffectFailure {
                        effect_id: planned.id,
                        message: source.to_string(),
                    },
                    source,
                });
            }
        }
        Ok(())
    }

    pub(crate) fn dispatch_committed_catalog_action(
        &mut self,
        invocation_id: InvocationId,
        logical_result: crate::action::outcome::ActionResult,
        effects: &[PlannedEffect],
    ) -> SettledInvocationKind {
        match self.apply_catalog_effects(effects) {
            Ok(()) => {
                self.catalog.settle(invocation_id, logical_result);
                SettledInvocationKind::Succeeded
            }
            Err(error) => {
                tracing::error!(
                    ?invocation_id,
                    effect = error.failure.effect_id.ordinal(),
                    source = %error.source,
                    "canonical action native effect failed"
                );
                self.catalog.degrade(invocation_id, vec![error.failure]);
                SettledInvocationKind::Degraded
            }
        }
    }

    fn apply_catalog_effect(&mut self, effect: &NativeEffect) -> eyre::Result<()> {
        match effect.clone() {
            NativeEffect::FocusNeighbor { direction } => {
                let focused_workspace = self.focused_workspace()?;
                match focused_workspace.layer {
                    WorkspaceLayer::Tiling => {
                        self.focus_container_in_direction(direction)?;
                    }
                    WorkspaceLayer::Floating => {
                        self.focus_floating_window_in_direction(direction)?;
                    }
                }
            }
            NativeEffect::MoveNeighbor { direction } => {
                let focused_workspace = self.focused_workspace()?;
                match focused_workspace.layer {
                    WorkspaceLayer::Tiling => {
                        self.move_container_in_direction(direction)?;
                    }
                    WorkspaceLayer::Floating => {
                        self.move_floating_window_in_direction(direction)?;
                    }
                }
            }
            NativeEffect::SetLayout { layout } => {
                self.change_workspace_layout_default(layout)?;
            }
            NativeEffect::SetWindowFloating { .. } => {
                self.toggle_float(false)?;
            }
            NativeEffect::Resize { axis, delta } => {
                let sizing = if delta.get() > 0 {
                    Sizing::Increase
                } else {
                    Sizing::Decrease
                };
                self.resize_window_on_axis(axis, sizing, delta.get().unsigned_abs() as i32)?;
            }
            NativeEffect::SetResizeStep { step } => {
                self.resize_step = step;
            }
            NativeEffect::SetTransparencyEnabled { enabled } => {
                transparency_manager::TRANSPARENCY_ENABLED.store(enabled, Ordering::SeqCst);
            }
            NativeEffect::SetTransparencyAlpha { alpha } => {
                transparency_manager::TRANSPARENCY_ALPHA.store(alpha.get(), Ordering::SeqCst);
            }
            NativeEffect::SetBorderEnabled {
                enabled,
                implementation,
            } => {
                match (enabled, implementation) {
                    (false, BorderImplementation::Komorebi) => {
                        border_manager::destroy_all_borders()?;
                    }
                    (false, BorderImplementation::Windows) => self.remove_all_accents()?,
                    (true, BorderImplementation::Komorebi) => {
                        border_manager::send_notification(None);
                    }
                    (true, BorderImplementation::Windows) => {}
                }
                border_manager::BORDER_ENABLED.store(enabled, Ordering::SeqCst);
            }
            NativeEffect::SetBorderColour {
                window_kind,
                colour,
            } => {
                let packed = colour.into();
                match window_kind {
                    WindowKind::Single => border_manager::FOCUSED.store(packed, Ordering::SeqCst),
                    WindowKind::Stack => border_manager::STACK.store(packed, Ordering::SeqCst),
                    WindowKind::Monocle => border_manager::MONOCLE.store(packed, Ordering::SeqCst),
                    WindowKind::Unfocused => {
                        border_manager::UNFOCUSED.store(packed, Ordering::SeqCst);
                    }
                    WindowKind::UnfocusedLocked => {
                        border_manager::UNFOCUSED_LOCKED.store(packed, Ordering::SeqCst);
                    }
                    WindowKind::Floating => {
                        border_manager::FLOATING.store(packed, Ordering::SeqCst);
                    }
                }
                border_manager::send_notification(None);
            }
            NativeEffect::SetBorderWidth { width } => {
                border_manager::BORDER_WIDTH.store(width.get(), Ordering::SeqCst);
                border_manager::send_notification(None);
            }
            NativeEffect::SetBorderOffset { offset } => {
                border_manager::BORDER_OFFSET.store(offset.get(), Ordering::SeqCst);
                border_manager::send_notification(None);
            }
            NativeEffect::SetBorderStyle { style } => {
                border_manager::STYLE.store(style);
                border_manager::send_notification(None);
            }
            NativeEffect::SetBorderImplementation { implementation } => {
                if implementation == BorderImplementation::Windows && !*WINDOWS_11 {
                    eyre::bail!("native Windows accent borders require Windows 11");
                }
                match implementation {
                    BorderImplementation::Komorebi => {
                        self.remove_all_accents()?;
                    }
                    BorderImplementation::Windows => border_manager::destroy_all_borders()?,
                }
                border_manager::IMPLEMENTATION.store(implementation);
                if implementation == BorderImplementation::Komorebi {
                    border_manager::send_notification(None);
                }
            }
            NativeEffect::SetStackbarMode { mode } => {
                stackbar_manager::STACKBAR_MODE.store(mode);
                self.retile_all(true)?;
                stackbar_manager::send_notification();
            }
            NativeEffect::SetStackbarLabel { label } => {
                stackbar_manager::STACKBAR_LABEL.store(label);
                stackbar_manager::send_notification();
            }
            NativeEffect::SetStackbarFocusedTextColour { colour } => {
                stackbar_manager::STACKBAR_FOCUSED_TEXT_COLOUR
                    .store(colour.into(), Ordering::SeqCst);
                stackbar_manager::send_notification();
            }
            NativeEffect::SetStackbarUnfocusedTextColour { colour } => {
                stackbar_manager::STACKBAR_UNFOCUSED_TEXT_COLOUR
                    .store(colour.into(), Ordering::SeqCst);
                stackbar_manager::send_notification();
            }
            NativeEffect::SetStackbarBackgroundColour { colour } => {
                stackbar_manager::STACKBAR_TAB_BACKGROUND_COLOUR
                    .store(colour.into(), Ordering::SeqCst);
                stackbar_manager::send_notification();
            }
            NativeEffect::SetStackbarHeight { height } => {
                stackbar_manager::STACKBAR_TAB_HEIGHT.store(height.get(), Ordering::SeqCst);
                self.retile_all(true)?;
                stackbar_manager::send_notification();
            }
            NativeEffect::SetStackbarTabWidth { width } => {
                stackbar_manager::STACKBAR_TAB_WIDTH.store(width.get(), Ordering::SeqCst);
                self.retile_all(true)?;
                stackbar_manager::send_notification();
            }
            NativeEffect::SetStackbarFontSize { size } => {
                stackbar_manager::STACKBAR_FONT_SIZE.store(size.get(), Ordering::SeqCst);
                stackbar_manager::send_notification();
            }
            NativeEffect::SetStackbarFontFamily { family } => {
                *stackbar_manager::STACKBAR_FONT_FAMILY.lock() = family;
                stackbar_manager::send_notification();
            }
            NativeEffect::SetAnimationEnabled { enabled, prefix } => match prefix {
                Some(prefix) => {
                    ANIMATION_ENABLED_PER_ANIMATION
                        .lock()
                        .insert(prefix, enabled);
                }
                None => {
                    ANIMATION_ENABLED_GLOBAL.store(enabled, Ordering::SeqCst);
                    ANIMATION_ENABLED_PER_ANIMATION.lock().clear();
                }
            },
            NativeEffect::SetAnimationDuration { duration, prefix } => match prefix {
                Some(prefix) => {
                    ANIMATION_DURATION_PER_ANIMATION
                        .lock()
                        .insert(prefix, duration.milliseconds());
                }
                None => {
                    ANIMATION_DURATION_GLOBAL.store(duration.milliseconds(), Ordering::SeqCst);
                    ANIMATION_DURATION_PER_ANIMATION.lock().clear();
                }
            },
            NativeEffect::SetAnimationFps { fps } => {
                set_animation_fps(fps);
            }
            NativeEffect::SetAnimationStyle { style, prefix } => match prefix {
                Some(prefix) => {
                    ANIMATION_STYLE_PER_ANIMATION.lock().insert(prefix, style);
                }
                None => {
                    *ANIMATION_STYLE_GLOBAL.lock() = style;
                    ANIMATION_STYLE_PER_ANIMATION.lock().clear();
                }
            },
            NativeEffect::CycleFocus { direction } => {
                let focused_workspace = self.focused_workspace()?;
                match focused_workspace.layer {
                    WorkspaceLayer::Tiling => {
                        self.focus_container_in_cycle_direction(direction)?;
                    }
                    WorkspaceLayer::Floating => {
                        self.focus_floating_window_in_cycle_direction(direction)?;
                    }
                }
            }
            NativeEffect::CycleMove { direction } => {
                self.move_container_in_cycle_direction(direction)?;
            }
            NativeEffect::ToggleMonocle => {
                self.toggle_monocle()?;
            }
            NativeEffect::ToggleMaximize => {
                self.toggle_maximize()?;
            }
            NativeEffect::ToggleLock => {
                self.toggle_lock()?;
            }
            NativeEffect::Stack { direction } => {
                self.add_window_to_container(direction)?;
            }
            NativeEffect::Unstack => {
                self.remove_window_from_container()?;
            }
            NativeEffect::StackAll => {
                self.stack_all()?;
            }
            NativeEffect::UnstackAll => {
                self.unstack_all(true)?;
            }
            NativeEffect::CycleStack { direction } => {
                self.cycle_container_window_in_direction(direction)?;
            }
            NativeEffect::CycleStackIndex { direction } => {
                self.cycle_container_window_index_in_direction(direction)?;
            }
            NativeEffect::FocusStack { index } => {
                if let Some(monitor_idx) = self.monitor_idx_from_current_pos() {
                    self.focus_monitor(monitor_idx)?;
                }
                self.focus_container_window(index.get())?;
            }
            NativeEffect::FocusWorkspace { index } => {
                self.focus_workspace_number(index.get())?;
            }
            NativeEffect::CycleFocusWorkspace { direction } => {
                self.cycle_focus_workspace(direction)?;
            }
            NativeEffect::CycleFocusEmptyWorkspace { direction } => {
                self.cycle_focus_empty_workspace(direction)?;
            }
            NativeEffect::FocusLastWorkspace => {
                self.focus_last_workspace()?;
            }
            NativeEffect::CloseWorkspace => {
                self.close_focused_workspace()?;
            }
            NativeEffect::FocusMonitor { index } => {
                self.focus_monitor_number(index.get())?;
            }
            NativeEffect::CycleFocusMonitor { direction } => {
                self.cycle_focus_monitor(direction)?;
            }
            NativeEffect::FocusMonitorAtCursor => {
                self.focus_monitor_at_cursor()?;
            }
            NativeEffect::FocusWorkspaceOnAllMonitors { index } => {
                self.focus_workspace_on_all_monitors(index.get())?;
            }
            NativeEffect::FocusMonitorWorkspace { monitor, workspace } => {
                self.focus_monitor_workspace(monitor.get(), workspace.get())?;
            }
            NativeEffect::CloseWindow => {
                self.close_foreground_window()?;
            }
            NativeEffect::MinimizeWindow => {
                self.minimize_foreground_window()?;
            }
            NativeEffect::ForceFocus => {
                self.force_focus_window()?;
            }
            NativeEffect::PromoteContainer => {
                self.promote_container_to_front()?;
            }
            NativeEffect::PromoteContainerSwap => {
                self.promote_container_swap()?;
            }
            NativeEffect::PromoteFocus => {
                self.promote_focus_to_front()?;
            }
            NativeEffect::PromoteWindow { direction } => {
                self.promote_window_in_direction(direction)?;
            }
            NativeEffect::CreateWorkspace => {
                self.new_workspace()?;
            }
            NativeEffect::ToggleTiling => {
                self.toggle_tiling()?;
            }
            NativeEffect::CycleLayout { direction } => {
                self.cycle_layout(direction)?;
            }
            NativeEffect::FlipLayout { axis } => {
                self.flip_layout(axis)?;
            }
            NativeEffect::ToggleWorkspaceLayer => {
                self.toggle_workspace_layer()?;
            }
            NativeEffect::MoveContainerToLastWorkspace => {
                self.move_container_to_last_workspace(true)?;
            }
            NativeEffect::SendContainerToLastWorkspace => {
                self.move_container_to_last_workspace(false)?;
            }
            NativeEffect::MoveContainerToWorkspace { index } => {
                self.move_container_to_workspace(index.get(), true, None)?;
            }
            NativeEffect::CycleMoveContainerToWorkspace { direction } => {
                self.cycle_move_container_to_workspace(direction, true)?;
            }
            NativeEffect::SendContainerToWorkspace { index } => {
                self.move_container_to_workspace(index.get(), false, None)?;
            }
            NativeEffect::CycleSendContainerToWorkspace { direction } => {
                self.cycle_move_container_to_workspace(direction, false)?;
            }
            NativeEffect::MoveContainerToMonitor { index } => {
                self.transfer_container_to_monitor(index.get(), None, true)?;
            }
            NativeEffect::CycleMoveContainerToMonitor { direction } => {
                self.cycle_transfer_container_to_monitor(direction, true)?;
            }
            NativeEffect::SendContainerToMonitor { index } => {
                self.transfer_container_to_monitor(index.get(), None, false)?;
            }
            NativeEffect::CycleSendContainerToMonitor { direction } => {
                self.cycle_transfer_container_to_monitor(direction, false)?;
            }
            NativeEffect::MoveContainerToMonitorWorkspace { monitor, workspace } => {
                self.transfer_container_to_monitor(monitor.get(), Some(workspace.get()), true)?;
            }
            NativeEffect::SendContainerToMonitorWorkspace { monitor, workspace } => {
                self.transfer_container_to_monitor(monitor.get(), Some(workspace.get()), false)?;
            }
            NativeEffect::MoveWorkspaceToMonitor { index } => {
                self.move_workspace_to_monitor(index.get())?;
            }
            NativeEffect::CycleMoveWorkspaceToMonitor { direction } => {
                self.cycle_move_workspace_to_monitor(direction)?;
            }
            NativeEffect::SwapWorkspacesToMonitor { index } => {
                self.swap_focused_monitor(index.get())?;
            }
            NativeEffect::PreselectDirection { direction } => {
                self.apply_preselect_direction(direction)?;
            }
            NativeEffect::CancelPreselect => {
                self.cancel_focused_preselect()?;
            }
            NativeEffect::Retile => {
                border_manager::destroy_all_borders()?;
                self.retile_all(false)?;
            }
            NativeEffect::RetileWithResizeDimensions => {
                border_manager::destroy_all_borders()?;
                self.retile_all(true)?;
            }
            NativeEffect::ManageFocusedWindow => {
                self.manage_focused_window()?;
            }
            NativeEffect::UnmanageFocusedWindow => {
                self.unmanage_focused_window()?;
            }
            NativeEffect::AdjustContainerPadding { sizing, adjustment } => {
                self.adjust_container_padding(sizing, adjustment)?;
            }
            NativeEffect::AdjustWorkspacePadding { sizing, adjustment } => {
                self.adjust_workspace_padding(sizing, adjustment)?;
            }
            NativeEffect::ToggleMouseFollowsFocus => {
                self.toggle_mouse_follows_focus();
            }
            NativeEffect::SetMouseFollowsFocus { enabled } => {
                self.set_mouse_follows_focus(enabled);
            }
            NativeEffect::ToggleWindowContainerBehaviour => {
                self.toggle_window_container_behaviour();
            }
            NativeEffect::ToggleFloatOverride => {
                self.toggle_float_override();
            }
            NativeEffect::ToggleWorkspaceWindowContainerBehaviour => {
                self.toggle_workspace_window_container_behaviour()?;
            }
            NativeEffect::ToggleWorkspaceFloatOverride => {
                self.toggle_workspace_float_override()?;
            }
            NativeEffect::ToggleCrossMonitorMoveBehaviour => {
                self.toggle_cross_monitor_move_behaviour();
            }
            NativeEffect::ToggleMonocleFocusBehaviour => {
                self.toggle_monocle_focus_behaviour();
            }
            NativeEffect::TogglePause => {
                self.toggle_pause()?;
            }
            NativeEffect::SetFocusedContainerPadding { size } => {
                self.set_focused_container_padding(size)?;
            }
            NativeEffect::SetFocusedWorkspacePadding { size } => {
                self.set_focused_workspace_padding(size)?;
            }
            NativeEffect::SetContainerPadding {
                monitor,
                workspace,
                size,
            } => {
                self.set_container_padding(monitor.get(), workspace.get(), size)?;
            }
            NativeEffect::SetWorkspacePadding {
                monitor,
                workspace,
                size,
            } => {
                self.set_workspace_padding(monitor.get(), workspace.get(), size)?;
            }
            NativeEffect::SetWorkspaceTiling {
                monitor,
                workspace,
                tile,
            } => {
                self.set_workspace_tiling(monitor.get(), workspace.get(), tile)?;
            }
            NativeEffect::SetMonitorWorkspaceLayout {
                monitor,
                workspace,
                layout,
            } => {
                self.set_workspace_layout_default(monitor.get(), workspace.get(), layout)?;
            }
            NativeEffect::EnsureWorkspaces { monitor, count } => {
                self.ensure_workspaces_for_monitor(monitor.get(), count)?;
            }
            NativeEffect::ClearWorkspaceLayoutRules { monitor, workspace } => {
                self.clear_workspace_layout_rules(monitor.get(), workspace.get())?;
            }
            NativeEffect::SetScrollingColumns { columns } => {
                self.set_scrolling_columns(columns)?;
            }
            NativeEffect::LockContainer {
                monitor,
                workspace,
                container,
            } => {
                self.set_container_locked(monitor.get(), workspace.get(), container.get(), true)?;
            }
            NativeEffect::UnlockContainer {
                monitor,
                workspace,
                container,
            } => {
                self.set_container_locked(monitor.get(), workspace.get(), container.get(), false)?;
            }
            NativeEffect::ToggleTitleBars => {
                self.toggle_title_bars()?;
            }
            NativeEffect::EnforceWorkspaceRules => {
                self.already_moved_window_handles.lock().clear();
                self.enforce_workspace_rules()?;
            }
            NativeEffect::AddSessionFloatRule => {
                self.add_session_float_rule()?;
            }
            NativeEffect::ClearSessionFloatRules => {
                self.clear_session_float_rules();
            }
            NativeEffect::ResizeEdge { direction, delta } => {
                let sizing = if delta.get() > 0 {
                    Sizing::Increase
                } else {
                    Sizing::Decrease
                };
                self.resize_window(direction, sizing, delta.get().unsigned_abs() as i32, true)?;
            }
            NativeEffect::SetWindowHidingBehaviour { behaviour } => {
                *HIDING_BEHAVIOUR.lock() = behaviour;
            }
            NativeEffect::SetCrossMonitorMoveBehaviour { behaviour } => {
                self.cross_monitor_move_behaviour = behaviour;
            }
            NativeEffect::SetMonocleFocusBehaviour { behaviour } => {
                self.monocle_focus_behaviour = behaviour;
            }
            NativeEffect::SetUnmanagedWindowOperationBehaviour { behaviour } => {
                self.unmanaged_window_operation_behaviour = behaviour;
            }
            NativeEffect::SetFocusFollowsMouse {
                implementation,
                enabled,
            } => {
                self.set_focus_follows_mouse_implementation(implementation, enabled)?;
            }
            NativeEffect::ToggleFocusFollowsMouse { implementation } => {
                self.toggle_focus_follows_mouse_implementation(implementation)?;
            }
            NativeEffect::AddWorkspaceLayoutRule {
                monitor,
                workspace,
                at_container_count,
                layout,
            } => {
                self.add_workspace_layout_default_rule(
                    monitor.get(),
                    workspace.get(),
                    at_container_count,
                    layout,
                )?;
            }
            NativeEffect::SetLayoutRatios { columns, rows } => {
                self.set_layout_ratios(columns.as_deref(), rows.as_deref())?;
            }
            NativeEffect::SetCustomLayout { path } => {
                self.change_workspace_custom_layout(path)?;
            }
            NativeEffect::SetWorkspaceCustomLayout {
                monitor,
                workspace,
                path,
            } => {
                self.set_workspace_layout_custom(monitor.get(), workspace.get(), path)?;
            }
            NativeEffect::AddWorkspaceCustomLayoutRule {
                monitor,
                workspace,
                at_container_count,
                path,
            } => {
                self.add_workspace_layout_custom_rule(
                    monitor.get(),
                    workspace.get(),
                    at_container_count,
                    path,
                )?;
            }
            NativeEffect::EnsureNamedWorkspaces { monitor, names } => {
                let names: Vec<String> =
                    names.into_iter().map(WorkspaceName::into_string).collect();
                self.ensure_named_workspaces_for_monitor(monitor.get(), &names)?;
            }
            NativeEffect::SetWorkspaceName {
                monitor,
                workspace,
                name,
            } => {
                self.set_workspace_name(monitor.get(), workspace.get(), name.into_string())?;
            }
            NativeEffect::EagerFocus { exe } => {
                self.eager_focus_exe(&exe)?;
            }
            NativeEffect::RemoveTitleBar { identifier, id } => {
                self.add_no_titlebar_rule(identifier, id);
            }
        }
        Ok(())
    }
}

fn observe_animation_configuration() -> AnimationConfiguration {
    let enabled = ANIMATION_ENABLED_PER_ANIMATION.lock().clone();
    let duration = ANIMATION_DURATION_PER_ANIMATION.lock().clone();
    let global_style = *ANIMATION_STYLE_GLOBAL.lock();
    let style = ANIMATION_STYLE_PER_ANIMATION.lock().clone();
    AnimationConfiguration {
        enabled: ScopedAnimationValue {
            global: ANIMATION_ENABLED_GLOBAL.load(Ordering::SeqCst),
            movement: enabled.get(&AnimationPrefix::Movement).copied(),
            transparency: enabled.get(&AnimationPrefix::Transparency).copied(),
        },
        duration: ScopedAnimationValue {
            global: AnimationDuration::new(ANIMATION_DURATION_GLOBAL.load(Ordering::SeqCst)),
            movement: duration
                .get(&AnimationPrefix::Movement)
                .copied()
                .map(AnimationDuration::new),
            transparency: duration
                .get(&AnimationPrefix::Transparency)
                .copied()
                .map(AnimationDuration::new),
        },
        style: ScopedAnimationValue {
            global: global_style.into(),
            movement: style
                .get(&AnimationPrefix::Movement)
                .copied()
                .map(AnimationStyleSnapshot::from),
            transparency: style
                .get(&AnimationPrefix::Transparency)
                .copied()
                .map(AnimationStyleSnapshot::from),
        },
        fps: animation_fps(),
    }
}

#[cfg(test)]
mod tests {
    use komorebi_protocol::InvocationNamespaceId;
    use komorebi_protocol::InvocationSequence;
    use komorebi_protocol::ManagerEpoch;
    use komorebi_protocol::Revision;
    use komorebi_protocol::StateStamp;

    use super::*;
    use crate::action::WorkspaceSelector;
    use crate::action::invoke::InvocationStatus;
    use crate::action::outcome::ActionResult;

    fn invocation(sequence: u64) -> InvocationId {
        InvocationId::new(
            InvocationNamespaceId::new([9; 16]).expect("test namespace is nonzero"),
            InvocationSequence::try_from(sequence).expect("test sequence is nonzero"),
        )
    }

    fn principal(byte: u8) -> PrincipalId {
        PrincipalId::new([byte; 32]).expect("test principal is nonzero")
    }

    fn empty_manager() -> WindowManager {
        WindowManager::new(ManagerEpoch::new([1; 16]).expect("test epoch is non-nil"))
            .expect("test manager should initialize")
    }

    #[test]
    fn successful_native_effect_settles_the_committed_invocation() {
        let mut manager = empty_manager();

        let invocation_id = manager
            .admit_socket_action(BuiltinAction::TogglePause)
            .expect("pause should apply without a monitor");

        assert!(manager.is_paused);
        assert_eq!(
            manager.catalog.status(invocation_id),
            Some(&InvocationStatus::Settled {
                state: manager.catalog.snapshot().state,
                result: ActionResult::PauseToggled { paused: true },
            })
        );
    }

    #[test]
    fn failed_native_effect_degrades_the_committed_invocation() {
        let mut manager = empty_manager();

        let error = manager
            .admit_socket_action(BuiltinAction::SetWorkspaceLayout {
                workspace: WorkspaceSelector::FocusedAtExecution,
                layout: DefaultLayout::Columns,
            })
            .expect_err("layout application without a monitor should fail");
        let invocation_id = error
            .invocation_id()
            .expect("native-effect failure has an invocation identity");
        let failure = match error {
            CatalogActionError::NativeEffects { failure, .. } => failure,
            CatalogActionError::Identity(_)
            | CatalogActionError::Observation { .. }
            | CatalogActionError::Rejected { .. } => {
                panic!("expected native-effect failure")
            }
        };
        assert_eq!(failure.effect_id.ordinal(), 0);

        assert_eq!(
            manager.catalog.status(invocation_id),
            Some(&InvocationStatus::Degraded {
                state: manager.catalog.snapshot().state,
                failures: vec![failure],
            })
        );
    }

    #[test]
    fn canonical_entrypoint_preserves_caller_identity_on_rejection() {
        let mut manager = empty_manager();
        let invocation_id = invocation(1);
        let context = InvocationContext {
            principal: principal(42),
            origin: InvocationOrigin::Palette,
            grants: ActionGrants::all(),
        };

        let error = manager
            .invoke_catalog_action(
                InvokeAction {
                    invocation_id,
                    expected_state: StateStamp::new(
                        manager.manager_epoch,
                        Revision::try_from(9).expect("test revision is nonzero"),
                    ),
                    action: BuiltinAction::TogglePause,
                    confirmation: None,
                },
                &context,
            )
            .expect_err("a stale caller revision should be rejected");

        assert_eq!(error.invocation_id(), Some(invocation_id));
        assert!(matches!(
            error,
            CatalogActionError::Rejected {
                source: ActionRejection::StaleState { .. },
                ..
            }
        ));
        assert!(!manager.is_paused);
        assert_eq!(
            manager.catalog.snapshot().state,
            StateStamp::initial(manager.manager_epoch)
        );
    }
}
