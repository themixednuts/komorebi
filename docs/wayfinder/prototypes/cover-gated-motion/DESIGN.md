# Production decision

Use cover-gated motion only through measured admission. A complete privacy-safe cover must be visible before native placement begins, and every runtime deadline remains authoritative. An unavailable, stale, or failing capability profile returns `SettleNow`; it never silently lowers a budget or guesses from a machine-wide constant.

The spike found a real capacity boundary: 49 simultaneous live thumbnails missed the 240 Hz pacing budget, while the same 50-subject presentation with 19 live thumbnails and 31 placeholders met it. That number describes this measured run only. Production derives admission from a generation-bound profile and current resource estimate instead of embedding `20` in policy.

## Primitive model

```rust
#[repr(transparent)]
pub struct RefreshHz(NonZeroU32);

#[repr(transparent)]
pub struct RefreshInterval(Duration);

#[repr(transparent)]
pub struct PresentationDeadline(Instant);

pub struct DisplayFingerprint {
    topology: DisplayTopologyGeneration,
    adapter: AdapterLuid,
    mode: DisplayMode,
    dpi: PhysicalDpi,
    hdr: HdrState,
}

pub struct MotionCapabilityProfile {
    display: DisplayFingerprint,
    backend: PresentationBackendGeneration,
    measured_at: MonotonicInstant,
    live_preview_budget: LivePreviewBudget,
    evidence: MotionBudgetEvidence,
}

pub enum PreviewPlan {
    Live(CurrentPreviewGeneration),
    Placeholder(PlaceholderReason),
}

pub enum MotionAdmission {
    Present(AdmittedMotionSequence),
    SettleNow(SettlementReason),
}

pub enum PresentationFailure {
    StaleGeneration,
    CoverDeadlineMissed,
    RendererUnavailable,
    DeviceLost,
    DisplayChanged,
    PreviewLost(WindowIdentity),
    NativeEffect(NativeEffectFailure),
}
```

Private constructors enforce nonzero refresh, physical-pixel geometry, matching topology/backend generations, current preview generations, complete monitor coverage, and deadlines derived from the observed refresh interval. `LivePreviewBudget` is an evidence result, not configuration. Protected, unavailable, stale, or excess previews become typed placeholders before the first frame.

The admission operation is pure:

```text
admit(sequence, current display fingerprint, capability profile, resource estimate)
  -> reject stale or mismatched evidence
  -> assign privacy-required placeholders
  -> admit live previews only while every measured budget remains satisfied
  -> prove complete cover accounting
  -> Present(admitted sequence) | SettleNow(reason)
```

## Entrypoint-to-effect stack

```text
untrusted hotkey / palette / Lua / protocol request / platform observation
  -> adapter parses a typed ActionInvocation or PlatformObservation
    -> OrderedOwner::submit
      -> pure transition commits manager intent and Revision
      -> motion::plan(before, after, effective policy)
      -> motion::admit(plan, MotionCapabilityProfile)
        -> SettleNow(reason)
          -> EffectExecutor::apply(NativeSettlementPlan)
        -> Present(sequence)
          -> MotionCoordinator::begin(sequence, CancellationToken)
            -> ShellMotionPort::prepare_cover(CoverFrame, deadline)
              -> bounded typed channel to the GUI presentation process
              -> GPUI monitor surface + narrow DWM-thumbnail adapter
            <- CoverPresented | CoverFailed | CoverDeadlineMissed
            -> EffectExecutor::apply(prepare_under_cover)
              -> BeginDeferWindowPos / DeferWindowPos / EndDeferWindowPos
            -> presenter samples immutable tracks from monotonic time
            -> EffectExecutor::apply(settle_at_end)
            -> WindowSystem::observe(final identities)
            -> presenter retires the cover by the hard deadline
          <- generation-fenced PresentationEvent and EffectOutcome
      -> ordered owner reconciles observed mismatch or records typed degradation
```

Raw `HWND`, HRESULT, GPUI, DWM, Lua, and channel types end at their adapters. The authoritative owner never waits for a frame. A presentation failure cannot roll back committed intent.

The Win32 adapter translates HRESULT and last-error values into `PresentationFailure` or `NativeEffectFailure`. The coordinator owns cover and retirement deadlines but never retries placement. The ordered owner owns reconciliation after uncertain native outcomes. Duplicate and stale outcomes are rejected by motion ID, generation, and revision; they are not retried by callers.

## Runtime and cancellation

Each production executable has one runtime boundary:

```rust
#[tokio::main]
async fn main() -> Result<(), StartupError> {
    run(ProcessCancellation::from_windows()).await
}
```

No library creates a runtime and application code never calls `block_on`. Every spawned Tokio task is owned by a process task group, receives an explicit cancellation token, and is joined during bounded shutdown. Cancellation is safe at every await: intent is already committed, effect requests are identified and idempotent, channel sends do not retain a half-mutated domain value, and dropping a future cannot skip native settlement or cover retirement.

User32 message pumps and COM apartments stay on dedicated affinity threads. They exchange generation-fenced requests and outcomes over bounded typed channels; a full channel returns overload or replaces only explicitly coalescible frame/pointer samples. It never drops button transitions, cancellation, settlement, privacy changes, or native outcomes.

## Deadline behavior

- `CoverPresented` is accepted only for the current topology, renderer, revision, and preview generation.
- Missing the two-refresh cover deadline selects immediate settlement and retires any partial surface.
- Frames sample monotonic time; missed frames are skipped and never stretch duration.
- Preview loss changes the affected subject to a placeholder on the next frame. Destruction of the native thumbnail handle occurs after visual replacement.
- Cancellation or supersession advances the generation, applies the newest required settlement, and retires all temporary resources.
- Fullscreen, HDR, mixed-DPI, multi-monitor, or renderer generations without matching evidence settle directly.

## Why the alternatives lose

Per-frame foreign `SetWindowPos` traffic violates grouped settlement and exposes application-specific redraw latency. A fixed live-thumbnail count turns one machine's measurement into policy and becomes wrong after a display, driver, renderer, or topology change. Treating `DwmFlush` timing metadata as the cover's presentation timestamp is also invalid: `DwmGetCompositionTimingInfo` reports compositor frames without attributing one to this surface. The probe therefore measures a conservative show/paint/flush/desktop-pixel upper bound and makes its limits explicit.

The two serious ownership choices were:

- Let the renderer decide how many previews to show. This is locally convenient but leaks privacy, budget, and degradation policy into GPUI and makes a renderer restart change behavior.
- Let the motion domain admit a renderer-neutral `PreviewPlan` from measured evidence. This keeps invalid or stale evidence out of the renderer and gives every caller the same immediate-settlement semantics.

The second wins because capability validity, privacy, and settlement are domain invariants. Evidence that would reverse it is a documented presentation API that atomically validates and schedules foreign content with its own enforceable privacy and deadline contract.

## Module ownership

- `komorebi/src/motion/model.rs`: refined IDs, generations, deadlines, fingerprints, preview plans, admission and failure values.
- `komorebi/src/motion/plan.rs`: pure before/after snapshot comparison and complete cover accounting.
- `komorebi/src/motion/admit.rs`: pure capability-profile validation and preview-budget assignment.
- `komorebi/src/motion/coordinator.rs`: current generation, cancellation, deadlines, supersession, and retirement.
- `komorebi/src/motion/ports.rs`: the consumer-owned `ShellMotionPort`; add no broader renderer service.
- `komorebi/src/effect/`: existing revisioned native settlement planning and outcome re-entry.
- shell GUI process: GPUI projection and monitor surfaces; it receives immutable admitted plans and owns no manager intent.
- Windows presentation adapter: raw DWM thumbnail lifecycle, surface handles, frame completion, and HRESULT translation.
- binary composition roots: the sole `#[tokio::main]` runtime, owned task group, bounded channels, affinity threads, and concrete adapter wiring.

## Vertical proof

- Enter through a real action adapter and prove acknowledgement/commit completes without waiting for a cover.
- Run pure admission properties across stale topology/backend/preview generations and prove the only result is `SettleNow` or a fully accounted cover.
- Drive a fake clock and presenter through every cancellation point; prove newest native settlement and cover retirement remain mandatory after future drop.
- Inject duplicate, late, and out-of-order renderer/effect outcomes; prove revision and generation fencing makes replay deterministic.
- Run the native 20/50 matrix at every exposed refresh mode with protected, unavailable, and destroyed previews.
- Terminate the shell process after every presentation phase and prove the manager converges without foreign opacity, hidden-window residue, detached tasks, or replayed animation state.
- Use full-frame capture plus ETW/GPU and process-resource tracing for the production GPUI backend; prove zero unsafe pixels and return to the idle resource band within 500 ms.
