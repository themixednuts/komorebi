# Passive desktop-observation contract

Windows virtual desktops are an external visibility domain. They are not manager workspaces and their undocumented ordering is never used as manager state.

## Primitive types

```rust
#[repr(transparent)]
struct WindowsDesktopId(u128);

#[repr(transparent)]
struct ObservationEpoch(u64);

#[repr(transparent)]
struct StableSampleCount(u8);

enum PublicObservation<T> {
    Value(T),
    Unavailable(ObservationFailure),
}

enum DesktopMembership {
    Current,
    Other,
}

enum ObservationFailure {
    WindowGone,
    AccessDenied,
    ShellUnavailable,
    UnsupportedHresult(i32),
}

struct WindowDesktopObservation {
    window: WindowId,
    desktop: PublicObservation<WindowsDesktopId>,
    membership: PublicObservation<DesktopMembership>,
    cloaked: PublicObservation<bool>,
}

struct CohortSnapshot {
    epoch: ObservationEpoch,
    windows: Vec<WindowDesktopObservation>,
}
```

`WindowsDesktopId` is opaque. Equality is useful; order and arithmetic are not. `PublicObservation` prevents an HRESULT, `GUID_NULL`, or missing HWND from becoming a false “other desktop” answer.

## Settling machine

```rust
enum DesktopSettlement {
    Establishing,
    Stable(StableCohort),
    Candidate {
        prior: StableCohort,
        next: CohortSnapshot,
        equal_samples: StableSampleCount,
    },
    ObservationUnavailable {
        prior: Option<StableCohort>,
        failure: ObservationFailure,
    },
}
```

- Three equal cohort snapshots establish or advance a stable generation.
- A sample equal to the prior stable cohort cancels a candidate.
- A different candidate replaces the previous candidate and resets its equal-sample count.
- Any unavailable cohort member makes the cohort unavailable. It never proves a desktop change.
- Explorer restart advances `ObservationEpoch`, discards candidates, recreates the public COM adapter, and requires a fresh stable baseline.

The final polling interval is selected from the measured 16, 100, and 500 millisecond runs. Equal-sample count remains three unless the switch traces show a multi-stage public observation lasting across three samples.

## Typed call stack

```text
WindowsEventLoop::on_idle_tick(now)
  -> DesktopObservationCoordinator::poll(now)
    -> PublicVirtualDesktopPort::observe(cohort)
      -> IVirtualDesktopManager::GetWindowDesktopId(hwnd)
      -> IVirtualDesktopManager::IsWindowOnCurrentVirtualDesktop(hwnd)
      -> DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED)
    <- Result<CohortSnapshot, AdapterFailure>
    -> DesktopSettler::ingest(snapshot)
    <- SettlementTransition
    -> DesktopVisibilityPlanner::plan(transition, prior_plan)
    <- VisibilityEffects
  -> ScratchpadCoordinator::apply(effects)
  -> WorkspacePresentation::refresh(effects)
```

The adapter owns HRESULT classification and COM recreation. The settler is pure and owns temporal evidence. The planner receives only settled transitions or explicit unavailability; it never calls Windows APIs.

## Effect rules

- `Stable -> Stable` with a new generation may suspend scratchpad presentation and refresh workspace navigation state.
- `Candidate` changes no window visibility.
- `ObservationUnavailable` preserves the last settled state, suppresses hide/show effects, and surfaces degraded observation state to diagnostics.
- Recovery requires a new three-sample stable cohort in the current epoch; stale evidence from a prior Explorer lifetime cannot complete it.

This contract observes public per-window facts only. It cannot enumerate desktops, name them, infer a supported ordering, create them, or switch them.
