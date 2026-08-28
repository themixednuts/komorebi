# AppBar lifecycle contract

## Alternatives

1. A timer watchdog could repeatedly query the work area and re-register when it looks wrong. It was rejected because a work-area snapshot does not identify registration ownership, fixed delays race Explorer startup, and two repair actors can oscillate.
2. The AppBar process could treat every `TaskbarCreated` or callback as an unconditional registration. It was rejected because duplicate `ABM_NEW` calls blur ownership and make callback ordering state-dependent.
3. The selected design fences registration by Shell generation and converts native callbacks into coalesced typed invalidations. Explorer owns work-area arbitration; the manager owns one registration and one queued position pass at a time.

The decision reverses only if a documented Windows 11 callback is shown to omit a required lifecycle transition. A missing callback is reported as degradation; it does not authorize background polling.

## Code-shaped contract

```rust
struct ShellGeneration {
    process_id: u32,
    created_100ns: u64,
}

enum Registration {
    Detached,
    Registering(ShellGeneration),
    Registered(ShellGeneration),
    Destroyed,
}

enum Position {
    Settled,
    Queued,
    Applying { invalidated: bool },
}

enum AppBarWake {
    ShellPositionChanged,
    ShellRecreated(ShellGeneration),
    DisplayChanged,
    DpiChanged(NonZeroU32),
    GeometryChanged(AppBarSpec),
    Shutdown,
}

enum AppBarEffect {
    Register(ShellGeneration),
    Position(AppBarSpec),
    Remove,
    ShowWithoutActivation,
    Destroy,
}
```

`Lifecycle` owns duplicate suppression and invalidation coalescing. `AppBarHost` owns UI-thread sequencing. The Windows adapter alone owns `SHAppBarMessage`, window messages, HWND values, and Shell error translation. The process supervisor owns restart after process death; an AppBar cannot watchdog its own crash.

## Startup to first frame

```text
appbar-child GUI entry: ChildOptions
  -> child::run: create hidden WS_POPUP HWND on the UI thread
    -> windows::shell_identity: ShellGeneration | QueryError
      -> model::Lifecycle::begin_registration: Register | AlreadyRegistered
        -> SHAppBarMessage(ABM_NEW): authoritative registration side effect
          -> Lifecycle::registration_succeeded
            -> PostMessage(POSITION_MESSAGE): one local deferred pass
              -> SHAppBarMessage(ABM_QUERYPOS)
                -> SHAppBarMessage(ABM_SETPOS)
                  -> MoveWindow
                    -> ShowWindow(SW_SHOWNOACTIVATE): first visible frame
```

Failure before `ShowWindow` destroys a still-hidden host. Registration failure returns to `Detached`; no caller retries on a timer.

## Native invalidation to convergence

```text
WndProc: ABN_POSCHANGED | WM_DISPLAYCHANGE | WM_DPICHANGED | geometry command
  -> dispatch_host_message: AppBarWake | DecodeError
    -> Lifecycle::request_position
      -> first wake: PostMessage(POSITION_MESSAGE)
      -> queued wake: merge cause, no second message
      -> applying wake: mark invalidated
        -> Host::position: one query/set/move pass
          -> Lifecycle::finish_position
            -> invalidated: post exactly one later POSITION_MESSAGE
            -> otherwise: Settled
```

`ABM_SETPOS` can precede Explorer's work-area publication. The later `ABN_POSCHANGED` is a new native invalidation and may therefore cause one further pass. No elapsed-time or rectangle-equality condition drives the state machine.

## Explorer restart

```text
Explorer replacement broadcasts TaskbarCreated
  -> WndProc registered message
    -> windows::shell_identity: replacement ShellGeneration
      -> Lifecycle::begin_registration
        -> same generation: RegistrationSuppressed
        -> new generation: ABM_NEW once, then queue ShellRecreated position
```

Process ID alone is not identity because Windows can reuse it. Creation time fences the generation.

## Graceful stop and crash

```text
Shutdown
  -> Lifecycle::detach
    -> SHAppBarMessage(ABM_REMOVE)
      -> DestroyWindow

Shell-role process handle becomes signaled
  -> manager supervisor native process wait
    -> start a fresh hidden shell-role process
```

If the process dies before `ABM_REMOVE`, Explorer releases its reservation and notifies surviving AppBars. The supervisor does not inspect or repair the work area.

## Production ownership

- `komorebi-shell/appbar/model.rs`: pure lifecycle and geometry types.
- `komorebi-shell/appbar/host.rs`: UI-thread orchestration and effect ordering.
- `komorebi-shell/windows/appbar.rs`: Win32 adapter and error translation.
- manager supervisor: shell-role process lifetime and native wait.
- renderer: pixels and accessibility only; it cannot register or reserve an edge.

Do not put this state machine into `komorebi-bar/src/bar.rs`; that file is already over 1,400 lines in the current worktree.
