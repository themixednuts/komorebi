# Interactive stacks prototype result

Artifacts: [interactive HTML](https://github.com/themixednuts/komorebi/blob/prototype/interactive-stacks/docs/wayfinder/prototypes/interactive-stacks/interactive-stacks-prototype.html), [browserless interaction checks](https://github.com/themixednuts/komorebi/blob/prototype/interactive-stacks/docs/wayfinder/prototypes/interactive-stacks/interaction-model.test.cjs), and [prototype branch](https://github.com/themixednuts/komorebi/tree/prototype/interactive-stacks/docs/wayfinder/prototypes/interactive-stacks).

## Decision

Use direct semantic targets on the destination container:

- center: stack at the end;
- top, right, bottom, left: split on that exact side;
- stackbar insertion slots: reorder or insert at an exact index;
- workspace background on another monitor: transfer as a new container;
- Escape, right-click, source loss, or an invalid release: cancel without changing manager intent.

Keep the direct-zone structure, add the action rail's explicit target label and rejection reason, and use the compass variant's keyboard navigator without its radial UI. Do not ship the rail or compass as alternative modes.

The direct variant needs one target acquisition and keeps spatial intent attached to the affected window. The rail needs a destination acquisition followed by an action acquisition. The compass obscures the destination, collapses on small tiles, and creates a second placement language without adding capability.

## Stackbar and lock behavior

The stackbar is the persistent representation of a multi-window container. Its tabs focus members when idle. During a placement session, insertion slots between tabs are exact reorder or insertion targets. It does not own stack state.

A locked container is a structural anchor:

- it cannot start a structural placement;
- it cannot accept stack membership or tab reorder changes;
- an unlocked window may split beside it because the locked container keeps its ordinal anchor;
- workspace relayout may still change its geometry;
- unlocking is an explicit action, never an implicit side effect of a drop.

## Shared pointer and keyboard behavior

Pointer and keyboard input create the same typed placement session and semantic target. Keyboard placement starts from the focused window, arrows choose a destination container, Tab cycles stack and split sides, Enter commits, and Escape cancels. The UI announces target changes and rejection reasons through an accessible live region.

Only pointer transitions are lossless. High-rate pointer motion is a latest-value sample: hit testing consumes the newest available physical point at presentation cadence, so a 1 kHz mouse cannot backlog the manager input lane.

## Primitive-first Rust shape

```rust
#[repr(transparent)]
pub struct DropSessionId(NonZeroU64);

#[repr(transparent)]
pub struct DropGeneration(NonZeroU64);

pub struct SourcePlacement {
    pub monitor: MonitorId,
    pub workspace: WorkspaceId,
    pub container: ContainerId,
    pub member: StackIndex,
}

pub enum PlacementOrigin {
    NativeTitlebarMove,
    StackbarTab,
    Keyboard,
}

pub struct DropSession {
    pub id: DropSessionId,
    pub generation: DropGeneration,
    pub expected_revision: StateRevision,
    pub source: WindowIdentity,
    pub source_placement: SourcePlacement,
    pub origin: PlacementOrigin,
}

pub enum SplitSide {
    Left,
    Top,
    Right,
    Bottom,
}

pub enum RawDropTarget {
    StackAt {
        monitor: MonitorId,
        workspace: WorkspaceId,
        container: ContainerId,
        index: StackIndex,
    },
    SplitBeside {
        monitor: MonitorId,
        workspace: WorkspaceId,
        anchor: ContainerId,
        side: SplitSide,
    },
    EmptyWorkspace {
        monitor: MonitorId,
        workspace: WorkspaceId,
    },
}

pub struct ValidatedDropTarget {
    target: RawDropTarget,
    expected_revision: StateRevision,
}

pub enum DropRejection {
    SourceGone,
    SourceLocked,
    TargetGone,
    TargetLocked,
    SamePlacement,
    StaleRevision,
    ModalConstraint,
    UnsupportedSurface,
}

pub enum PlacementInput {
    Begin(BeginPlacement),
    Preview(PreviewPlacement),
    Commit(CommitPlacement),
    Cancel(CancelPlacement),
}
```

`ValidatedDropTarget` has private fields. Only the pure placement policy can construct it after checking window identity, manager revision, monitor/workspace/container existence, lock policy, modal constraints, and target legality. Toolkits receive a renderer-neutral `DropPreviewView`, not mutable domain objects.

## Typed call stacks

### Native titlebar drag

```text
WinEventAdapter::move_size_start(window)
  -> ManagerInput::BeginPlacement(window, NativeTitlebarMove)
  -> PlacementPolicy::begin(&AuthoritativeState, request)
  -> DropSession
  -> ManagerTransition::commit(PlacementStarted)
  -> ShellSnapshot::drop_preview()
  -> GpuiProjection::present_drop_targets()
```

Windows continues to render the foreign window during its native move loop. The manager adds owned target overlays; it does not mutate foreign opacity, subclass the foreign window, or pretend to replace DWM.

### Pointer preview

```text
InputService::latest_pointer_sample()
  -> DropTargetIndex::hit_test(PhysicalPoint)
  -> PlacementPolicy::preview(&AuthoritativeState, &DropSession, RawDropTarget)
  -> Result<ValidatedDropTarget, DropRejection>
  -> DropPreviewView
  -> GpuiProjection::update_drop_targets()
```

Pointer samples may coalesce. Session begin, button release, cancellation, and commit remain ordered input transitions.

### Keyboard preview

```text
BindingResolver::resolve(trigger, captured_context)
  -> ActionInvocation<BeginKeyboardPlacement | MoveDropTarget | CycleDropKind>
  -> PlacementCoordinator::apply(...)
  -> PlacementPolicy::preview(...)
  -> the same DropPreviewView
```

### Commit and native effects

```text
MoveSizeEnd | ButtonReleased | ActionInvocation<CommitPlacement>
  -> ManagerInput::CommitPlacement(session_id, generation, target, expected_revision)
  -> PlacementPolicy::revalidate(&AuthoritativeState, request)
  -> Result<ValidatedDropTarget, DropRejection>
  -> ManagerTransition::apply(PlacementCommitted)
  -> EffectPlan::from_committed_topology()
  -> Win32PlacementAdapter::apply(final HWND geometry/visibility)
  -> EffectOutcome
  -> ManagerInput::ObserveEffectOutcome
  -> Reconciliation
```

The topology commits before native effects. A rejection leaves manager intent unchanged and closes the overlay. If Windows already moved a foreign HWND during a native drag, reconciliation converges it back to committed geometry; the UI does not claim that no platform motion occurred.

### Cancellation and target loss

```text
Escape | RightButton | SourceDestroyed | SecureDesktop | GenerationAdvanced
  -> ManagerInput::CancelPlacement(session_id, generation, reason)
  -> PlacementCoordinator::cancel_if_current()
  -> PlacementCancelled
  -> ShellSnapshot without drop preview
```

A destination disappearing or the state revision advancing is a rejected commit, not a best-effort retarget. The source topology stays committed and the accessible status explains that the user can retry.

## Windows-specific limits

- DWM remains the compositor. The manager can own overlay windows and final placement effects, not the pixels or move loop of arbitrary foreign windows.
- A native titlebar move physically moves the foreign HWND before komorebi receives `EVENT_SYSTEM_MOVESIZEEND`. Cancellation may therefore require a convergent move back to committed geometry.
- Cross-monitor hit testing starts in physical desktop coordinates, then resolves a concrete monitor and that monitor's work area/DPI. It does not assume one logical coordinate scale.
- A stackbar tab drag may use a DWM thumbnail or privacy-safe placeholder because no foreign titlebar move loop is active. Preview generation and privacy rules from the composition decision still apply.
- Dialogs and unresolved modal families are not structural drop sources or targets.

## Verification

`node interaction-model.test.cjs` passes deterministic checks for stacking, one-revision commit, exact tab reorder, requested split side, locked-source and locked-target rejection, cancellation, cross-monitor placement, stale-target rejection, revision-race rejection, and keyboard parity. The embedded script also parses independently.

The Codex in-app browser blocked navigation to the local `file://` URL, so no automated screenshot or coordinate-level visual test was used. This is a browser-control limitation, not evidence about the interaction model. The HTML remains directly openable for human visual inspection.
