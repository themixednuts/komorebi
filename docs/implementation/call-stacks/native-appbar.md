# Native AppBar call stack

## Ownership

The shell owns AppBar registration and work-area reservation. GPUI owns pixels
and input only. No renderer may call `SHAppBarMessage`, retain a raw `HWND`, or
repair the work area with a timer.

`AppBarLifecycle` is the pure state machine. Its non-clonable
`RegistrationAttempt` and `PositionPass` values prove which native completion
is legal. `ShellGeneration` combines Explorer's nonzero process ID and process
creation time so PID reuse cannot suppress required re-registration.

`AppBarHost<P>` is the renderer-neutral coordinator. `P` implements the narrow
`AppBarHostPlatform` effects: identify Explorer, register, enqueue one position
message, atomically position-and-optionally-reveal, remove, and accept new
geometry. Renderers receive no lifecycle state and cannot reorder these effects.

`WindowsAppBarApi` is the only `SHAppBarMessage` adapter. GPUI exposes its
native window through `HasWindowHandle`; the UI thread converts that borrowed
handle to `BorrowedAppBarWindow` for the duration of a native call. The borrow
is thread-affine, and the adapter never owns or destroys the GPUI window.

`WindowsAppBarBinding` installs a documented comctl32 window subclass. The
callback classifies only registered AppBar messages, `ABN_POSCHANGED`,
`WM_DISPLAYCHANGE`, `WM_DPICHANGED`, and `WM_NCDESTROY`; every other message is
forwarded through `DefSubclassProc`. Native notifications received during an
AppBar call only set typed pending flags and post one coalesced wake message.
The later wake owns all mutable host access, so synchronous Win32 reentrancy
cannot alias Rust state.

## Registration

```text
hidden GPUI bar HWND
  -> AppBarHost::start
    -> query current ShellGeneration
    -> AppBarLifecycle::begin_registration
    -> AlreadyRegistered | Destroyed: no effect
    -> Register(RegistrationAttempt)
      -> SHAppBarMessage(ABM_NEW)
        -> failure: registration_failed(attempt)
        -> success: registration_succeeded(attempt)
          -> invalidate_position
            -> post one private position message
```

`WindowsAppBarApi::shell_generation` obtains Explorer's PID from
`GetShellWindow`, opens the process with query-only access, and combines the PID
with `GetProcessTimes` creation time. Its owned process handle closes on every
return path.

The first visible frame follows successful `ABM_QUERYPOS`, `ABM_SETPOS`, and
window positioning. This prevents a bar flash at an unnegotiated rectangle.

## Native invalidation

```text
ABN_POSCHANGED | WM_DISPLAYCHANGE | WM_DPICHANGED | geometry update
  -> WindowsAppBarBinding: set pending signal and post one registered wake
  -> AppBarHost::position_event_received
  -> AppBarLifecycle::invalidate_position
    -> Schedule: post one private position message
    -> Coalesced: no second message

private position message
  -> AppBarHost::position_requested
  -> begin_position -> PositionPass
    -> WindowsAppBarApi::reserve
      -> SHAppBarMessage(ABM_QUERYPOS)
    -> AppBarGeometry::apply_thickness
      -> SHAppBarMessage(ABM_SETPOS)
    -> SetWindowPos(SWP_NOACTIVATE)
    -> first pass only: ShowWindow(SW_SHOWNOACTIVATE)
    -> finish_position(PositionPass)
      -> Settled
      -> ScheduleAgain: post exactly one follow-up
```

An `ABN_POSCHANGED` arriving during a native position call is retained as one
follow-up pass. No elapsed time, rectangle comparison, or continuous polling
drives convergence.

## Shutdown and Explorer replacement

`TaskbarCreated` causes a new Shell generation query. The same generation is
suppressed; a changed generation receives exactly one `ABM_NEW`. Graceful
shutdown consumes the proof carried by `RegistrationRemoval::Remove` to issue
one `ABM_REMOVE` before destroying the window. Success permanently destroys the
lifecycle; failure restores the registered state so an explicit shutdown retry
can converge. Process supervision owns crash restart; the AppBar does not
watchdog itself.

## Proven model invariants

- Empty physical rectangles and zero thickness cannot be constructed.
- Thickness is clamped to the negotiated monitor axis without coordinate
  overflow.
- One Explorer generation registers once; a replacement generation registers
  again.
- Duplicate and reentrant position invalidations converge to at most one queued
  follow-up pass.
- Destroy reports exactly one native removal and permanently closes future
  registration.
