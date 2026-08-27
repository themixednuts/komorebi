# Results

Measured on August 27, 2026, with Windows 11 Home 25H2, build `26200.9168`, on `x86_64-pc-windows-msvc`. The probe used Rust `1.97.1` and `windows` `0.62.2`.

## Injected outcomes

`cargo run -- fake` exercised all four outcomes at each native effect boundary.

| Boundary | Applied | Rejected | TimedOut | Unknown |
| --- | --- | --- | --- | --- |
| Workspace focus | observed target settled | observed mismatch planned focus convergence | later observation found the effect applied | later observation found the effect applied |
| Foreign window movement | observed target settled | observed mismatch planned geometry convergence | later observation found the effect applied | later observation found the effect applied |
| Manager-owned shell window | observed target settled | intent returned to the unchanged captured frame | exact captured frame restored through a second effect | exact captured frame restored through a second effect |

All 12 cases passed. Each case had one acknowledgement per input, revisioned events with `InputId` causation, and deterministic replay of state, acknowledgements, and events. A command using stale revision `99` against revision `0` returned a typed `StaleRevision` rejection, planned no effects, and changed no state.

The fake treats `TimedOut` and `Unknown` as outcomes where Windows may have applied the requested mutation. That is the dangerous branch. Observation settles foreign-window intent, while the manager-owned shell path restores its exact captured frame.

## Live Win32

`cargo run -- live` created two temporary ordinary HWNDs and one hidden topmost tool HWND. It used real `SetForegroundWindow`, `SetWindowPos`, `GetForegroundWindow`, `GetWindowRect`, `IsWindow`, `IsWindowVisible`, and `DestroyWindow` calls.

| Scenario | Result | Evidence |
| --- | --- | --- |
| Workspace focus | pass | `SetForegroundWindow` returned an applied outcome, and observation identified test window 2 as foreground. |
| Window movement | pass | The requested and observed frame matched exactly. |
| Externally moved HWND | pass | An out-of-band move changed the observed frame; the next transition planned one convergent move and restored intended geometry. |
| Externally destroyed HWND | pass | Observation marked test window 2 destroyed, cleared its focus intent, and left no native work targeting it. |
| Manager-owned shell window | pass | Visibility and frame matched the requested `SurfaceFrame`. |
| Exact shell restoration | pass | The captured hidden frame at `250,80 420x64` was restored exactly. |
| Stale command | pass | Revision `0` against current revision `16` returned `StaleRevision` and planned no effect. |

The run committed 16 revisions from 17 inputs. The seventeenth input was the rejected stale command. It produced 17 acknowledgements, deterministic replay passed, every committed event had a valid revision and cause, and the divergence list was empty.

Windows chose the initial frames of the ordinary test windows rather than preserving the coordinates passed at creation. The probe correctly seeded authoritative observations from `GetWindowRect` before planning its first command. This is evidence against treating requested creation geometry as Windows truth.

Observation failure has a distinct representation from destruction. A failed `GetWindowRect` becomes `WindowUnavailable` and preserves intent while marking the observation uncertain; only `IsWindow == false` becomes `WindowDestroyed`. The shell adapter follows the same rule and marks the surface degraded instead of inventing a zero frame.

## Limits

The live probe used responsive windows on the same thread. It could not produce a real `TimedOut` result, exercise hung foreign UI threads, or prove deadline behavior for cross-thread asynchronous positioning. The fake matrix proves the state transition for those outcomes, not the Windows adapter that detects them.

The probe also does not test multi-monitor DPI changes, process crashes between commit and effect reporting, durable journal recovery, bounded queue overload, or real komorebi workspace extraction. Those belong in production design and vertical tests after this decision, not in this disposable slice.

The result supports the single-owner, commit-before-effect architecture. Production extraction still needs an owned effect executor, bounded non-dropping input lanes, revisioned snapshot resynchronization, a startup reconciliation pass, and a durable recovery record for exact manager-owned restoration.
