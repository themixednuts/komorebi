# Passive desktop-observation contract

Windows virtual desktops are an external visibility domain. They are not manager workspaces, and their undocumented ordering is never used as manager state.

## Primitive types

```rust
#[repr(transparent)]
struct WindowsDesktopId(u128);

#[repr(transparent)]
struct ObservationEpoch(u64);

#[repr(transparent)]
struct StableSampleCount(u8);

enum DesktopWakeSource {
    DesktopAccessibilityNameChanged,
    ManagedWindowCloaked(WindowId),
    ManagedWindowUncloaked(WindowId),
    ShellEpochChanged,
}

struct DesktopWake {
    source: DesktopWakeSource,
    observed_at: MonotonicInstant,
}

enum RequiredObservation<T> {
    Value(T),
    Unavailable(ObservationFailure),
}

enum SupplementalObservation<T> {
    Value(T),
    NotAssigned,
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
    membership: RequiredObservation<DesktopMembership>,
    desktop: SupplementalObservation<WindowsDesktopId>,
    cloaked: SupplementalObservation<bool>,
}

struct CohortSnapshot {
    epoch: ObservationEpoch,
    windows: Vec<WindowDesktopObservation>,
}
```

`WindowsDesktopId` is opaque. Equality is useful; ordering and arithmetic are not. Desktop identity is supplemental because valid HWNDs can return `TYPE_E_ELEMENTNOTFOUND` or `GUID_NULL` while membership remains available. Those outcomes never become a false `Other` value.

## Event-first settling machine

```rust
enum DesktopObservationMode {
    Establishing {
        latest: Option<CohortSnapshot>,
        equal_samples: StableSampleCount,
    },
    Dormant {
        stable: StableCohort,
    },
    Settling {
        prior: StableCohort,
        wake: DesktopWake,
        latest: CohortSnapshot,
        equal_samples: StableSampleCount,
        deadline: MonotonicInstant,
    },
    ObservationUnavailable {
        prior: Option<StableCohort>,
        failure: ObservationFailure,
    },
}
```

- Startup immediately samples until three equal snapshots establish the first stable cohort.
- `Dormant` performs no periodic desktop queries.
- A qualified WinEvent creates one `Settling` generation and arms an immediate sample followed by 16 ms samples.
- Repeated wakes coalesce into the active generation and never run visibility logic inside the callback.
- Three equal cohort snapshots commit the generation and return to `Dormant`, even if the final cohort equals the prior cohort. The wake itself represents an observed shell transition, which matters when all managed windows are pinned.
- A different snapshot replaces the candidate and resets its equal-sample count.
- A required membership failure enters `ObservationUnavailable`. Supplemental GUID or cloak failures remain explicit fields but do not discard valid membership facts.
- A 500 ms deadline bounds the burst. Expiry preserves the prior stable cohort and reports degraded observation.
- Explorer restart advances `ObservationEpoch`, discards candidates, recreates the public COM adapter, and requires a fresh stable baseline.

## Typed call stack

```text
WinEventCallback::on_event(raw_event)
  -> WindowsDesktopWakeAdapter::classify(raw_event, desktop_hwnd, managed_hwnds)
  <- Option<DesktopWake>
  -> ManagerEventSender::try_send(ManagerEvent::WindowsDesktopWake(wake))

ManagerLoop::on_windows_desktop_wake(wake)
  -> DesktopObservationCoordinator::wake(wake)
    -> BurstScheduler::arm_now_then_every(16ms, deadline = now + 500ms)

ManagerLoop::on_desktop_sample_due(now)
  -> PublicVirtualDesktopPort::observe(managed_windows)
    -> IVirtualDesktopManager::IsWindowOnCurrentVirtualDesktop(hwnd)
    -> IVirtualDesktopManager::GetWindowDesktopId(hwnd)
    -> DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED)
  <- Result<CohortSnapshot, AdapterFailure>
  -> DesktopSettler::ingest(snapshot)
  <- SettlementTransition
  -> BurstScheduler::disarm_if_settled(transition)
  -> DesktopVisibilityPlanner::plan(transition, prior_plan)
  <- VisibilityEffects
  -> ScratchpadCoordinator::apply(effects)
  -> WorkspacePresentation::refresh(effects)
```

The WinEvent adapter is a narrow unsafe boundary around `SetWinEventHook`. It subscribes out of context, filters `EVENT_OBJECT_NAMECHANGE` to the current desktop HWND, filters cloak events to managed HWNDs, and posts a typed event. The callback does not call COM or manager logic because WinEvent delivery is reentrant.

The public adapter owns HRESULT classification and COM recreation. The settler is pure and owns temporal evidence. The planner receives only settled transitions or explicit unavailability; it never calls Windows APIs.

## Effect rules

- A committed generation may suspend scratchpad presentation and refresh workspace navigation state.
- `Settling` changes no window visibility.
- `ObservationUnavailable` preserves the last settled state, suppresses hide/show effects, and surfaces degraded observation state to diagnostics.
- Recovery requires a new three-sample stable cohort in the current epoch. Evidence from a prior Explorer lifetime cannot complete it.

This contract observes public per-window facts only. It cannot enumerate desktops, name them, infer a supported ordering, create them, or switch them.
