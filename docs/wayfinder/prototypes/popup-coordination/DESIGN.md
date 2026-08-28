# Popup coordination prototype design

This disposable prototype answers [Measure third-party popup observation and coordination](https://github.com/themixednuts/komorebi/issues/42). It emits JSON evidence and never becomes the production popup subsystem. The user's no-interactive-HTML rule overrides the prototype skill's usual HTML shell.

## Behaviors and constraints

- Observe WinEvent hints without querying foreign windows, allocating an unbounded collection, blocking, sleeping, or polling in the callback.
- Build one owned observation from documented Win32 facts. Treat UI Automation as bounded supplementary evidence.
- Classify a surface and its family through a pure exhaustive operation. Missing, contradictory, stale, or denied evidence stays unresolved and never means safe to tile.
- Guard only family-affecting actions when modal state is active or unresolved.
- Center or recover only controlled test windows. Preserve size, activation, z-order, owner order, styles, topmost state, and parentage.
- Rebuild from a full census after reordered, duplicated, missing, or delayed hints. Raw HWND reuse cannot inherit an earlier incarnation.
- Redact titles completely. Keep paths as native Windows values inside the process and emit only non-authoritative display labels or digests.
- Enter Tokio through one `#[tokio::main]` boundary. The macro expands to one runtime builder and `block_on`; never nest a runtime or synchronously wait on a runtime worker.
- Make every await boundary cancellation-safe. Owned hooks and child processes converge through drop guards; capacity is reserved before channel commits; generations reject late results; evidence publication uses replace-on-success.

## Runtime alternatives

### UI Automation on a replaceable thread

A dedicated COM MTA thread is the smallest normal client topology and Microsoft recommends it instead of a UI thread. It cannot meet the fault boundary, however. Rust and Windows provide no safe operation that terminates one blocked thread and runs all Rust destructors. A timed-out provider call would leak a stuck thread, its COM apartment, and any retained resources.

### UI Automation in a sacrificial process

A manager-owned child process contains one MTA and receives only a native window reference plus a deadline generation. The parent waits on the child process handle. At the deadline it terminates and reaps that child, marks the fact unavailable, and starts a fresh generation only for later work. Startup cost is off the manager owner loop and must be measured.

The prototype compares responsive thread and process latency, then injects a provider that blocks `WM_GETOBJECT`. The process design wins unless the fault injection shows a documented bounded UI Automation call or safe thread reclamation.

## Typed contracts

```rust
pub fn classify_surface(observation: &SurfaceObservation) -> SurfaceDecision;
pub fn guard_family(constraint: &ModalConstraint, action: FamilyAction) -> GuardDecision;
pub fn plan_placement(request: PlacementRequest) -> Result<Option<PlacementPlan>, PlacementUnavailable>;
impl FamilyModel {
    pub fn apply_hint(&mut self, hint: ObservationHint);
}

pub fn observe_window(window: NativeWindowRef, generation: u64) -> anyhow::Result<Win32Observation>;
pub async fn probe_process(executable: &Path, request: UiaRequest) -> UiaOutcome;
pub fn apply_controlled(window: ControlledWindow, plan: PlacementPlan) -> anyhow::Result<Win32Observation>;
```

`NativeWindowRef`, `HWND`, `RECT`, style words, process handles, UI Automation interfaces, HRESULT values, and UTF-16 buffers stay below native adapters. Domain code receives stable incarnations, typed physical-pixel rectangles, owner links, availability, and semantic evidence.

## Entrypoint-to-effect stacks

### Hint to classification

```text
WinEventProc: raw event fields
  -> event::publish: ObservationHint | filtered
     [hook thread, hot-local, bounded ArrayQueue::push]
    -> observer::drain_wake
       [owned worker, one pass per event wake]
      -> native::observe_window: Win32Observation | ObserveError
         [foreign read-only Win32 calls]
      -> uia::probe: UiaOutcome
         [sacrificial child process, MTA, process-handle deadline]
      -> domain::classify_surface: SurfaceDecision
         [pure; missing facts remain explicit]
      <- revisioned decision and explanation
```

Queue saturation increments a gap generation and returns from the callback. The observer schedules a full census; it does not replay guesses from dropped hints.

### Controlled placement

```text
test policy: CenterOnOwner
  -> domain::plan_placement: PlacementPlan | PlacementUnavailable
     [pure physical-pixel calculation]
    -> native::place_controlled
       [revalidate target and owner incarnation]
      -> SetWindowPos
         [ASYNCWINDOWPOS | NOACTIVATE | NOSIZE | NOZORDER | NOOWNERZORDER]
      <- native return plus fresh observation
    -> compare pre/post invariants
  <- AppliedAndObserved | Rejected | Unknown
```

The adapter makes one request. An ambiguous return causes observation, never a blind retry. The prototype restores its own test window through the same constrained effect.

### Modal action guard

```text
family-affecting action: FamilyAction
  -> domain::guard_family(ModalConstraint, FamilyAction)
    -> Allowed
    -> Rejected(ActiveDialog)
    -> Rejected(UnresolvedFacts)
  <- typed availability and reason
```

The workspace, scratchpad, close, minimize, and desktop callers do not duplicate modal rules. Actions on unrelated families never consult this family.

### UI Automation timeout

```text
observer: UiaRequest { window, generation, deadline }
  -> spawn uia-worker child in an owned Job
    -> child CoInitializeEx(COINIT_MULTITHREADED)
      -> IUIAutomation::ElementFromHandle and WindowPattern reads
    <- JSON UiaFacts and process exit
  -> WaitForSingleObject(process, deadline)
    -> signaled: validate generation and decode
    -> timeout: terminate/reap child and return UiaUnavailable::TimedOut
```

The manager owner never waits here. A late or previous-generation result cannot reenter authoritative state.

## Vertical proof

- Controlled native root, modal dialog, modeless utility, no-activate transient, menu, tooltip, combo popup, drag visual, and hung provider.
- Read-only census of currently present WinUI, Chromium/Electron, Java/Swing, shell, and other application windows without titles.
- Real out-of-context WinEvent saturation and callback histogram.
- Property tests for event reorder, duplication, loss, restart census, owner cycles, HWND reuse, stale generations, role-to-mode safety, modal action guards, and placement arithmetic.
- One controlled placement and one foreground request, followed by fresh observations and invariant comparison.
- Responsive and hung UI Automation calls under thread/process topology.
