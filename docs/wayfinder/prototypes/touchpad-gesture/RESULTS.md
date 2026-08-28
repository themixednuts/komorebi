# Touchpad gesture prototype, device-gated result

Status: partial evidence for [Prototype touchpad command integration and foreground gestures](https://github.com/themixednuts/komorebi/issues/14). This does not resolve that ticket. [Attach a Windows Precision Touchpad test device](https://github.com/themixednuts/komorebi/issues/44) now blocks the physical run.

Artifacts: [prototype branch](https://github.com/themixednuts/komorebi/tree/prototype/touchpad-gesture/docs/wayfinder/prototypes/touchpad-gesture), [native Rust probe](https://github.com/themixednuts/komorebi/tree/prototype/touchpad-gesture/docs/wayfinder/prototypes/touchpad-gesture/native-probe), and [gesture-session prototype](https://github.com/themixednuts/komorebi/blob/prototype/touchpad-gesture/docs/wayfinder/prototypes/touchpad-gesture/gesture-session-prototype.html).

## Current machine result

Measured on 2026-08-27 on Windows 11 Home 25H2, build 26200.9168.

The native Rust probe used `windows` 0.62.2 to query three independent signals:

| Signal | Result |
| --- | --- |
| Raw Input HID devices | 8 HID collections, none with Digitizers/Touch Pad usage `0x0D/0x05` |
| Pointer devices | 0 devices, therefore 0 `POINTER_DEVICE_TYPE_TOUCH_PAD` devices |
| `TouchpadGesturesController.IsSupported` | `true` |

`IsSupported` proves that this Windows build exposes the WinRT gesture contract. It does not prove that a physical device exists. PnP also lists only two present HID-compliant mice. The correct machine verdict is `no_present_precision_touchpad`.

The probe does not register handlers, enable a gesture controller, inject input, edit the registry, or change Windows Settings. Run it with:

```powershell
cargo run --manifest-path native-probe/Cargo.toml
```

## Behavior chosen pending calibration

Use two separate routes. They meet different contracts and must not masquerade as one gesture system.

### Discrete global commands

Let Windows own global three- and four-finger recognition. The owner may map chosen swipes to reserved keyboard chords in Windows Advanced touchpad gestures. The first-party input authority resolves those chords to ordinary command-catalog actions.

This path has completion only. It offers no progress, velocity, cancellation preview, or proof that a chord came from the touchpad. The physical run must determine whether Windows marks the generated chord as injected and whether the low-level hook and `RegisterHotKey` observe it consistently. Until then, do not broaden the normal injected-input policy.

The manager never edits undocumented Precision Touchpad registry values. Its setup flow may open `ms-settings:devices-touchpad`, show the exact reserved chords, and verify that each direction produces exactly one low-risk workspace action.

### Continuous foreground navigation

Use two-finger horizontal pan only inside the foreground overview window. Register that owned HWND as touchpad-capable, read its `WM_POINTER` touchpad frames, and translate them at the Win32 adapter into logical displacement and velocity. Vertical intent remains overview scrolling. Three- and four-finger shell gestures remain Windows-owned.

The pure logic prototype settles these rules:

- bind a session to the overview surface generation, monitor, starting workspace, and manager revision;
- treat the first frames as priming because Windows may delay gesture disambiguation;
- lock direction after horizontal or vertical intent wins by hysteresis;
- preview at most the adjacent workspace, never skip several workspaces from velocity;
- do not wrap at a monitor-local workspace edge and never transfer the session to another monitor;
- commit once on a valid release, or cancel on pointer cancellation, foreground loss, surface replacement, device loss, desktop switch, or stale manager revision;
- keep the same decision under reduced motion, but replace card translation with a static candidate highlight and immediate settlement.

The prototype's 35% displacement threshold, 12% minimum flick displacement, and normalized velocity are discussion values. They are not production constants. The physical run must calibrate in logical units and compare slow pans, flicks, and unintended motion.

Open `gesture-session-prototype.html` directly to drive deliberate commits, short snap-backs, flicks, vertical yield, focus loss, monitor binding, workspace edges, and reduced motion. The reducer is isolated from the DOM.

## Designs compared

### Selected: Windows global recognition plus foreground two-finger pan

Windows owns global gestures and conflict policy. The foreground overview owns only the input delivered to its own opted-in window. This keeps one global gesture owner, preserves three- and four-finger shell behavior unless the owner explicitly remaps it, and gives the overview enough contact data for continuous local preview.

### Rejected as the default: process-wide `TouchpadGesturesController`

The controller can expose continuous three-or-more-contact data, but Windows considers it only while its process is foreground and ignores background controllers. Enabling it also replaces the system handler for supported gestures during that foreground interval. The API is still documented as prerelease, and process scope is broader than the overview window. It adds conflict risk without giving the manager global progress.

Hardware evidence could revive this route only if two-finger window messages cannot produce a good overview interaction and a bounded enable/disable run proves zero duplicate or leaked shell actions.

### Rejected: direct background HID interpretation

Reading or reverse-engineering Precision Touchpad reports behind the Windows gesture stack would create a second global recognizer and conflict with Windows. Raw Input is not a supported route for consuming global PTP gestures from an always-background manager.

## Primitive-first Rust contract

```rust
#[repr(transparent)]
pub struct GestureGeneration(NonZeroU64);

#[repr(transparent)]
pub struct LogicalDip(i32);

#[repr(transparent)]
pub struct LogicalDipPerSecond(i32);

#[repr(transparent)]
pub struct GestureProgress(i16); // private constructor, -10_000..=10_000

pub struct ForegroundGestureContext {
    pub generation: GestureGeneration,
    pub surface: SurfaceIdentity,
    pub surface_generation: SurfaceGeneration,
    pub monitor: MonitorId,
    pub starting_workspace: WorkspaceId,
    pub expected_revision: StateRevision,
}

pub enum LockedAxis {
    Horizontal,
    Vertical,
}

pub enum ForegroundGesture {
    Priming {
        context: ForegroundGestureContext,
        baseline: ContactBaseline,
    },
    Tracking {
        context: ForegroundGestureContext,
        axis: LockedAxis,
        progress: GestureProgress,
        velocity: LogicalDipPerSecond,
        candidate: Option<WorkspaceId>,
    },
    YieldedToOverviewScroll {
        context: ForegroundGestureContext,
    },
}

pub enum GestureInput {
    ContactsBegan(ContactFrame),
    ContactsChanged(ContactFrame),
    ContactsReleased(ContactFrame),
    PointerCancelled,
    ForegroundLost,
    SurfaceReplaced,
    DeviceRemoved,
    DesktopChanged,
}

pub enum GestureOutput {
    Prime,
    Preview(WorkspacePreview),
    ScrollOverview(LogicalDip),
    Invoke(ActionInvocation),
    Cancel(GestureCancellation),
}

pub enum GestureCancellation {
    BelowThreshold,
    PointerCancelled,
    ForegroundLost,
    StaleSurface,
    StaleManagerRevision,
    DeviceRemoved,
    DesktopChanged,
    WorkspaceEdge,
}
```

Private constructors convert device units into logical DIPs, clamp progress, and require a nonzero session generation. `ForegroundGesture` makes a candidate impossible before direction lock. A release can produce at most one `ActionInvocation` because the reducer consumes the active state.

The Windows adapter owns `HWND`, `WM_POINTER`, `POINTER_TOUCHPAD_INFO`, history buffers, cancellation flags, device units, and Win32 errors. The reducer and command catalog never see them. The GPUI projection receives `WorkspacePreview` and `GestureCancellation`, not contact frames.

## Typed call stacks

### Discrete global swipe

```text
physical three- or four-finger swipe
  -> Windows Precision Touchpad stack and user-owned Advanced gesture mapping
    -> reserved keyboard chord
      -> low-level keyboard adapter: RawKeyTransition + InputOrigin
        -> compiled binding map: Result<ActionInvocation, BindingRejection>
          -> authoritative manager input lane
            -> catalog admission at expected manager revision
              -> workspace transition commits
                -> typed HWND placement EffectPlan
                  -> Win32 placement adapter
                <- EffectOutcome
              <- ActionSettlement
```

The first-party input authority owns deduplication and binding conflicts. Windows owns gesture recognition. The physical run must identify the generated event flags before the binding compiler decides which injected origin, if any, this exact route accepts. Same-user input injection cannot be authenticated as a touchpad, so this route is limited to ordinary low-risk actions.

### Foreground overview pan

```text
overview HWND receives WM_POINTER frame [UI thread, hot-local]
  -> touchpad Win32 adapter reads current frame and bounded history
    -> ContactFrame | TouchpadReadError
      -> pure overview gesture reducer
        -> Prime | ScrollOverview | Preview | Invoke | Cancel
          -> renderer-neutral overview state owner
            -> immutable OverviewSnapshot
              -> active GPUI projection

GestureOutput::Invoke(ActionInvocation) [one per consumed release]
  -> authoritative manager input lane
    -> revision and workspace admission
      -> committed workspace transition
        -> composition and HWND EffectPlan
          -> owned DWM-thumbnail projection plus Win32 placement adapter
        <- EffectOutcome
      <- ActionSettlement
    -> next OverviewSnapshot or typed cancellation
```

The UI thread may read the bounded current frame, run the pure reducer, and publish a preview. It cannot wait on manager IPC or native window effects. Vertical output updates the overview's own scrolling; it does not also forward a second gesture to Windows.

### Cancellation

```text
WM_POINTER cancellation | foreground loss | desktop switch | device removal
  -> typed GestureInput
    -> reducer consumes the current generation
      -> GestureOutput::Cancel(reason)
        -> overview clears preview and returns to committed workspace
        -> manager receives no ActionInvocation
```

No cancellation path fabricates a release. Work queued under the consumed generation is stale and cannot commit.

## Proposed ownership

- `input::touchpad_shortcut` in the first-party input authority owns the reserved-chord policy and event-origin classification. The process selected by [Choose manager process and Windows adapter boundaries](https://github.com/themixednuts/komorebi/issues/19) will host it.
- `overview::gesture` owns the pure foreground state and threshold policy.
- `shell::windows::touchpad` owns touchpad-capable HWND registration, frame reads, unit conversion, and Win32 error translation.
- The GPUI overview projection owns pixels and accessibility announcements only.
- The authoritative manager still owns workspace selection and native effects.

## Physical completion contract

After a Precision Touchpad is present:

1. Re-run the native probe and identify the concrete HID or pointer device, contact capacity, and API support.
2. Capture baseline Windows gestures before remapping. Then map two reserved chords and run at least 50 alternating physical swipes per direction. Record hook flags, misses, duplicates, repeats, conflicting shell actions, and behavior while ordinary and elevated windows are foreground.
3. In the foreground overview, capture slow pans, flicks, reversals, short releases, vertical motion, Windows-cancelled streams, Alt+Tab, overview close, device removal, and desktop switch. Record frame cadence, first-to-second-frame gap, history depth, cancellation flags, and decision outcome.
4. Repeat the overview run on each monitor and at each attached DPI. Prove that a session stays bound to its starting monitor and never wraps at an edge.
5. Repeat with reduced motion. The selected workspace and cancellation outcome must match the ordinary presentation.
6. Put the overview in the background before a gesture and during a gesture. No new background session may start; losing foreground cancels the active one.

Acceptance requires zero duplicate actions, zero commits after cancellation, one or zero actions per physical gesture, no simultaneous Windows and manager action, and identical decisions with reduced motion. Report cadence, latency, event-origin flags, and final thresholds remain unclaimed until this run exists.

## Sources

- [TouchpadGesturesController](https://learn.microsoft.com/en-us/windows/win32/input-precisiontouchpad/touchpadgesturescontroller)
- [RegisterTouchpadCapableWindow and RegisterTouchpadCapableThread](https://learn.microsoft.com/en-us/windows/win32/input-precisiontouchpad/registertouchpadcapable)
- [Windows Precision Touchpad HID collection](https://learn.microsoft.com/en-us/windows-hardware/design/component-guidelines/touchpad-windows-precision-touchpad-collection)
- [Touch gestures for Windows](https://support.microsoft.com/en-us/windows/hardware/input-devices/touch-gestures-for-windows)
- [Touchpad experience customization](https://learn.microsoft.com/en-us/windows-hardware/design/component-guidelines/touchpad-experience-customization)
