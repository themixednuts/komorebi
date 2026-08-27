# Revisioned manager loop prototype

This disposable prototype asks whether the state-and-effect model accepted in [Choose manager state ownership and native effect boundaries](https://github.com/themixednuts/komorebi/issues/20) survives uncertain Windows behavior.

It has two parts:

- `revisioned-manager-loop-prototype.html` is the human-driven model. Open it directly and use the walkthroughs or free-play controls.
- The Rust program runs an injected-outcome matrix and a reversible live Win32 pass with temporary windows.

Nothing here is production code. The branch preserves the prototype as evidence for [Prototype the revisioned manager loop and native effect recovery](https://github.com/themixednuts/komorebi/issues/36).

## Run it

Open the HTML file in a browser, or run the Rust probes from this directory:

```powershell
cargo run -- fake
cargo run -- live
```

The live mode briefly creates two ordinary windows and one manager-owned tool window. It restores the tool window, destroys all test windows, and attempts to restore the prior foreground window before exit. It never moves an existing application window.

## Prototype contract

`src/model.rs` is a pure state transition module. `OrderedOwner` is the only mutable owner. Every submitted `InputEnvelope` receives an `Acknowledgement`; accepted inputs create one revision and revisioned `CommittedEvent` values. `plan_transition` does not call Windows.

The model keeps these values separate:

- manager intent and Windows observations;
- commands, platform observations, and native effect outcomes;
- a committed transition and its later native work;
- exact restoration of a captured manager-owned shell frame and reconciliation of foreign windows.

`src/native.rs` owns the effect adapters. `WindowSystem` and `ShellSurfaceHost` consume domain values and translate Windows results into `Applied`, `Rejected`, `TimedOut`, or `Unknown`. Failed observations become typed `WindowUnavailable` or `ShellSurfaceUnavailable` inputs; they are never confused with a destroyed window or a zero-sized surface. Raw `HWND` values do not enter `src/model.rs`.

## Selected call stack

```text
typed command or platform observation
  -> OrderedOwner::submit
    -> plan_transition, pure and synchronous
      -> validate expected revision and domain identity
      -> commit AuthoritativeState at one new Revision
      -> append CommittedEvent values with InputId causation
      -> return Acknowledgement plus NativeEffect values
    -> WindowSystem or ShellSurfaceHost adapter
      -> Win32 effect
      <- EffectOutcome
    -> OrderedOwner::submit EffectReported
    -> observe Windows through the same narrow port
    -> OrderedOwner::submit PlatformObservation
      -> settle, plan convergence, or cancel work for a destroyed HWND
```

For an ambiguous manager-owned shell effect, `EffectReported` plans a `ShellPurpose::Restore` effect using the exact captured `SurfaceFrame`. For a foreign window, the owner observes Windows and plans convergence if observed geometry or focus differs from retained intent.

## Designs compared

The current direct design calls Win32 while `Arc<Mutex<WindowManager>>` is held. A slow application can extend the lock duration, and a partial native mutation has no committed outcome record. It also makes deterministic replay impossible because planning and effects are mixed.

The prototype commits logical intent before native work and feeds every outcome back through the same ordered owner. This design passed the fake and live scenarios, so it is the selected direction.

Staging a transition until every native effect succeeds looks attractive but does not fit Windows. Foreign windows can move, hang, or disappear between calls, and a multi-window operation is not an atomic transaction. This design would keep the manager lock tied to application behavior and still could not promise rollback.

Evidence that would reverse the selected direction is a live workload where commit-before-effect causes an unrecoverable visible state that an effect-first design can avoid without blocking the owner. The prototype found no such case.

## Files

- `src/model.rs` contains domain values, transition planning, the ordered owner, immutable snapshots, event causation, and replay.
- `src/native.rs` contains the fake and Win32 ports. Unsafe code is limited to the Win32 adapter.
- `src/main.rs` runs the fake matrix and live scenarios.
- `RESULTS.md` records the measured result and limitations.
