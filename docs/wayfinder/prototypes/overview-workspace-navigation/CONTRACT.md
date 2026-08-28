# Overview behavior contract

## Product boundary

The manager overview owns manager workspaces, containers, stacks, scratchpads, and exact move targets. Windows Task View remains the owner of creating, naming, ordering, and switching Windows virtual desktops. The overview may report the current public desktop-membership state, but it does not reproduce Task View's private virtual-desktop controls.

The initial production shape is one owned overview host per monitor. Each host uses that monitor's physical bounds, DPI, and refresh cadence. A session coordinator supplies one immutable overview snapshot and commits one manager intent. This avoids a single cross-DPI mega-window, keeps hit testing local, and matches the already-proven per-monitor transition surface.

Rejected alternative: one virtual-screen-sized host. It simplifies cross-monitor drawing, but spans unrelated DPI and refresh domains, makes partial monitor failure harder to contain, and creates one large topmost occlusion surface. Cross-monitor dragging does not require it because pointer coordinates and semantic targets already use physical desktop coordinates.

## Primitive-first model

```rust
pub struct OverviewSessionId(NonZeroU64);
pub struct OverviewGeneration(NonZeroU64);

pub struct OverviewSnapshot {
    pub session: OverviewSessionId,
    pub generation: OverviewGeneration,
    pub revision: StateRevision,
    pub monitors: Vec<OverviewMonitor>,
    pub focused: WindowIdentity,
    pub windows_desktop: WindowsDesktopObservation,
}

pub enum PreviewContent {
    Pending { generation: OverviewGeneration },
    LiveDwm { generation: OverviewGeneration, source: WindowIdentity },
    Placeholder { reason: PreviewUnavailable },
}

pub enum OverviewIntent {
    FocusWindow(WindowIdentity),
    FocusWorkspace { monitor: MonitorId, workspace: WorkspaceId },
    MoveWindow { window: WindowIdentity, target: OverviewDropTarget },
    ToggleScratchpad(ScratchpadId),
    Close,
}

pub enum OverviewDropTarget {
    Container { monitor: MonitorId, workspace: WorkspaceId, container: ContainerId },
    Workspace { monitor: MonitorId, workspace: WorkspaceId },
    Scratchpad(ScratchpadId),
}
```

Toolkit widgets never carry an `HWND`, mutate topology, or activate a process. Stable identities cross the projection boundary; row indexes and screen rectangles do not.

## Typed call stacks

### Open and first complete frame

```text
ActionInvocation<OpenOverview>
  -> OverviewCoordinator::open(&AuthoritativeState, ForegroundSnapshot)
  -> OverviewSnapshot
  -> OverviewHostPort::create_per_monitor(snapshot.monitors)
  -> PreviewPort::register_dwm_or_placeholder(window identities, generation)
  -> OverviewProjection::present_complete(snapshot, preview states)
  -> PresentedFrame(generation)
  -> OverviewCoordinator::mark_interactive(generation)
```

Hosts are non-visible until a complete first frame contains either current-generation content or a labeled placeholder for every slot. No real window is hidden or moved to open the overview.

### Focus a window

```text
PointerClick | KeyboardEnter
  -> OverviewInputAdapter::focus_window(stable identity, captured generation)
  -> OverviewPolicy::validate(&AuthoritativeState, request)
  -> Result<ValidatedFocus, OverviewRejection>
  -> OverviewHostPort::dismiss_without_activation()
  -> ForegroundPort::activate_from_user_input(window)
  -> ActivationOutcome
  -> ManagerInput::ObserveActivationOutcome
```

The overview closes before activation so it cannot remain above the target. `SetForegroundWindow` failure is an explicit outcome. The manager neither flashes the taskbar intentionally nor loops focus calls.

### Drag to a manager destination

```text
PointerDown(window identity)
  -> OverviewPolicy::begin_move(snapshot revision, generation)
  -> OverviewMoveSession
PointerSample(latest physical point)
  -> MonitorTargetIndex::hit_test(point)
  -> OverviewPolicy::preview(session, semantic target)
  -> OverviewProjection::show_drop_target()
PointerReleased
  -> OverviewPolicy::revalidate(authoritative state, session, target)
  -> ManagerTransition::apply(MoveWindow)
  -> EffectPlan::from_committed_topology()
  -> Win32PlacementAdapter::apply()
```

High-rate pointer samples coalesce to the newest physical point. Begin, release, cancellation, and commit remain ordered. DWM thumbnails are visual resources, not interactive child windows.

### Content loss

```text
SourceDestroyed | DwmRegistrationFailed | ContentWatchdogUnavailable
  -> PreviewPort::invalidate(source, generation)
  -> PreviewContent::Placeholder(reason)
  -> OverviewProjection::replace_by_next_frame()
```

Registration success and non-zero source size do not prove visible content. Protected, black, stale, minimized-unavailable, or destroyed content never falls through to a privacy-bypassing capture path.

## Acceptance contract

- Opens with no desktop-colored intermediate frame.
- First complete frame is presented within two refresh intervals at p95 for 20 and 50 windows on the target machine.
- Pointer and keyboard can reach the same window, workspace, container, and scratchpad actions.
- Drag targets are semantic manager targets and revalidate the manager revision at commit.
- Protected or unavailable content becomes a labeled placeholder by the next frame.
- Activation is attempted once from the actual user input path; failure leaves a clear recoverable state.
- Escape or focus loss closes without manager-state mutation.
- DWM thumbnails are unregistered on close, cancellation, source loss, monitor loss, and device recovery.
