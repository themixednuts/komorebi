# Spatial overview behavior contract

## Product boundary

The manager ships one spatial overview, not selectable overview modes. It preserves monitor placement, manager-workspace order, container and stack structure, scratchpads, and exact placement targets. Directional keyboard navigation and explicit selection details are part of that same surface.

Windows Task View remains responsible for creating, naming, ordering, switching, and removing Windows virtual desktops. The overview may show the manager's public observation of a window's Windows desktop visibility domain, but it never calls private virtual-desktop interfaces. A target outside the current visibility domain is identified and redirected to Task View rather than activated.

One interactive shell session creates one hidden owned host per monitor. Per-monitor hosts keep DPI, refresh, failure, and DWM-thumbnail ownership local. The session coordinator owns selection and cross-monitor placement, so a virtual-screen-sized window is unnecessary.

## Primitive-first model

```rust
pub struct OverviewSessionId(NonZeroU64);
pub struct OverviewGeneration(NonZeroU64);
pub struct PreviewSlotId(NonZeroU64);

pub struct OverviewSnapshot {
    pub session: OverviewSessionId,
    pub generation: OverviewGeneration,
    pub revision: StateRevision,
    pub monitors: OverviewMonitors,
    pub initial_selection: OverviewNodeId,
    pub focus_return: Option<FocusReturnTarget>,
    pub windows_desktop: WindowsDesktopObservation,
}

pub enum OverviewNodeId {
    Workspace { monitor: MonitorId, workspace: WorkspaceId },
    Container { monitor: MonitorId, workspace: WorkspaceId, container: ContainerId },
    Window(WindowIdentity),
    Scratchpad(ScratchpadId),
}

pub enum PreviewContent {
    Placeholder { reason: PreviewUnavailable },
    Live { slot: PreviewSlotId },
}

pub enum OverviewActivation {
    FocusWindow(WindowIdentity),
    FocusWorkspace { monitor: MonitorId, workspace: WorkspaceId },
    ToggleScratchpad(ScratchpadId),
}

pub enum OverviewRejection {
    StaleGeneration,
    StaleRevision,
    TargetGone,
    OutsideCurrentWindowsDesktop,
    ModalConstraint,
    InputAuthorityLost,
}
```

`OverviewMonitors` has a private representation and a validating constructor. It rejects duplicate monitor, workspace, container, window, and scratchpad identities. `initial_selection` must name a node in the same snapshot. This concentrates snapshot validation at the manager-to-shell boundary.

`PreviewSlotId` is renderer-neutral. Only the Win32 preview adapter maps it to an `HTHUMBNAIL`; neither the manager nor GPUI stores that handle. Toolkit widgets carry stable semantic identities, never row indexes, screen rectangles, raw `HWND`s, or effect closures.

## Typed call stacks

### Open without a flash

```text
ActionInvocation<OpenOverview>
  -> Manager::prepare_overview(&AuthoritativeState, ForegroundSnapshot)
  -> Result<PreparedOverview, OverviewOpenRejection>
  -> InteractiveShellRole::acquire(OverviewSessionId)
  -> OverviewHostPort::create_hidden(PerMonitorHostSpec)
  -> OverviewProjection::first_frame(all slots = current-generation placeholders)
  -> ToolkitFramePort::submit(CompleteOverviewFrame)
  -> FramePresented(OverviewGeneration)
  -> OverviewHostPort::show_presented_hosts()
  -> PreviewPort::promote_eligible_slots(OverviewGeneration)
  -> OverviewSession<Interactive>
```

The hosts stay hidden until the toolkit reports that the complete first frame was presented. Registration and live pixels are not on the opening critical path. There is no desktop-colored intermediate window and no fixed delay.

### Promote and invalidate a live preview

```text
PreviewPromotionRequested(PreviewSlotId, WindowIdentity, OverviewGeneration)
  -> OverviewCoordinator::validate_current_generation()
  -> Win32PreviewAdapter::register(destination HWND, source HWND, stable rectangle)
  -> Result<NativePreviewLease, PreviewUnavailable>
  -> PreviewLeaseTable::bind(PreviewSlotId, NativePreviewLease)
  -> OverviewProjection::replace_placeholder(PreviewContent::Live)

SourceDestroyed | SourceCloaked | MonitorRemoved | SessionClosed
  -> PreviewLeaseTable::take_matching(PreviewSlotId, OverviewGeneration)
  -> NativePreviewLease::drop() -> DwmUnregisterThumbnail
  -> OverviewProjection::replace_with_placeholder_if_session_is_current()
```

WinEvent, display-change, manager-event, and renderer callbacks wake these stacks. DWM updates live thumbnail pixels itself. No timer samples window state, foreground state, desktop membership, or thumbnail readiness.

### Directional keyboard navigation

```text
KeyDown(ArrowKey)
  -> OverviewInputAdapter::direction()
  -> OverviewNavigator::neighbor(current node, Direction, SnapshotGeometry)
  -> Option<OverviewNodeId>
  -> OverviewSession::select_if_current(generation, node)
  -> OverviewProjection::publish_selection_and_details()
```

The neighbor function is pure. It considers candidates in the requested half-plane, then orders by primary-axis distance, perpendicular distance, and stable semantic identity. Pointer and UI Automation selection use the same `OverviewNodeId` path.

### Activate a selection

```text
PointerClick | KeyboardEnter | UIA Invoke
  -> OverviewInputAdapter::activate(OverviewNodeId, OverviewGeneration)
  -> OverviewPolicy::validate_activation(&AuthoritativeState, snapshot revision)
  -> Result<OverviewActivation, OverviewRejection>
  -> Manager::commit(OverviewActivation)
  -> Transition + EffectPlan
  -> OverviewHostPort::dismiss_before_foreground_effect()
  -> EffectExecutor::apply_workspace_visibility_and_placement()
  -> ForegroundPort::activate_once_from_current_user_input()
  -> ActivationOutcome
  -> ManagerInput::EffectCompleted(ActivationOutcome)
```

The manager commits any manager-workspace change before deriving effects. It then dismisses the topmost overview and makes at most one foreground request. Windows may deny foreground activation; denial is an explicit recoverable outcome, not a retry trigger. A target in another Windows desktop visibility domain returns `OutsideCurrentWindowsDesktop` before mutation.

### Place a window by dragging

```text
PointerDown(WindowIdentity, OverviewGeneration)
  -> PlacementPolicy::begin(snapshot revision, stable source)
  -> Result<PlacementSession, PlacementRejection>

RawPointerMoved(PhysicalPoint)
  -> LatestPointerSample::replace()
  -> OverviewInputQueue::schedule_once_if_idle()
  -> PlacementTargetIndex::resolve(latest point)
  -> Option<PlacementTarget>
  -> OverviewProjection::show_semantic_target()

PointerReleased
  -> PlacementPolicy::revalidate(&AuthoritativeState, session, target)
  -> Manager::commit(PlaceWindow)
  -> Transition + EffectPlan
  -> Win32PlacementAdapter::apply()
```

Only replaceable pointer samples coalesce. Begin, release, cancellation, and commit remain ordered input transitions. Screen coordinates discover a revision-bound `PlacementTarget`; they never cross the manager transition boundary as the target itself. DWM thumbnails are visual resources, not hit-testable child windows.

## Text, path, and identity boundaries

Win32 window text enters as its original `Box<[u16]>`. Projection may derive a fallible or replacement-glyph `DisplayLabel`, but display text never becomes window identity and is never sent back to Windows. `WindowIdentity`, not a title, carries operational authority.

No filesystem path crosses this overview stack. Goal-wide file features must keep paths as `Path`/`PathBuf` or `OsStr`/`OsString`, preserving Rust's Windows WTF-8 semantics until a wide Win32 call. The standard-library types remain the identity type; `normpath` is permitted only at a boundary that explicitly requires resolved normalization, because verbatim input is intentionally left unchanged. Display strings are one-way views. Code must not use `to_string_lossy`, split paths on `/` or `\\`, rewrite UNC/verbatim/device prefixes, or inspect a path and later mutate it when a handle-relative operation can remove that time-of-check/time-of-use gap.

## Proven envelope and explicit limitations

- The native probe measured 20 and 50 visible DWM slots. Fifty is a proof point, not a hard product cap. Larger snapshots present complete placeholders first and promote visible, selected, and nearby slots as capacity permits.
- DWM thumbnail geometry stays stable while a card is live. GPUI or DirectComposition animates owned selection, dimming, and target visuals around it. Production does not call `DwmFlush` per frame.
- A successful DWM registration does not report whether pixels are black or stale. The manager keeps labels and actions useful but does not inspect pixels or bypass protection with alternate capture APIs.
- Public Windows APIs do not provide Task View's complete virtual-desktop lifecycle. The manager does not guess or bind to undocumented COM interfaces.
- Foreground activation is policy-controlled by Windows and cannot be guaranteed from arbitrary background state.

## Production acceptance contract

- No scheduled polling timer exists in the overview, foreground, preview, or Windows-desktop paths.
- Opening shows no desktop-colored or partially populated intermediate frame.
- On the target machine, p95 invocation-to-first-complete-frame is at most two refresh intervals with 20 and 50 represented windows; this must be measured in the production GPUI host because the native probe measures DWM work, not toolkit presentation.
- Pointer, keyboard, and UI Automation reach the same window, workspace, container, and scratchpad actions.
- Drag commits only a generation- and revision-valid semantic `PlacementTarget`.
- Known privacy denial, failed registration, destroyed source, and invalidated source produce a labeled placeholder without alternate capture.
- Escape, shell-role loss, monitor loss, and focus loss close without manager-state mutation unless an action already committed.
- Every native thumbnail lease is unregistered on replacement, close, cancellation, source loss, monitor loss, and device recovery.
- Activation is attempted once from current user input; failure is reported without taskbar flashing, synthetic input, thread attachment, or retry loops.
