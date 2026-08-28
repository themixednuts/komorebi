# Thermo-nuclear code-quality audit

## Verdict

The production architecture is approved with one condition: the raw D3D11/DirectComposition implementation remains disposable evidence and must not become a parallel renderer. The typed core and final call stack are small enough to implement without a service hierarchy.

## Structural findings resolved

- Renderer ownership was ambiguous while two visual planes existed. The decision now names GPUI's Windows scene as the sole production resource and composition owner.
- The original effect shape bundled a border into every instance and allowed zero identities. Closed parameter variants, non-zero identities, checked generations, explicit lifetime, leases, and pure aggregate admission now make those invalid states unrepresentable.
- Per-instance limits were not enough to stop aggregate exhaustion. `SceneBudget::admit` now uses checked arithmetic and returns a new value without side effects.
- The first GPUI spike used `unwrap`/`expect` in executable paths. Boundary construction now returns errors; report and window failures are surfaced instead of panicking.
- The DWM probe contained a mutation branch that is unreachable on this target because the documented set-only attribute cannot be read. The final design rejects foreign mutation rather than disguising an unknown baseline as restorable state.
- Shader source format, runtime format, and plugin authority were conflated. The design separates offline authoring from validated runtime DXBC and gives Lua only a digest-bound asset identity.
- Continuous animation had been easy to confuse with observation polling. The contract now requests renderer frames only while an admitted effect is active and uses native signals for readiness/lifecycle state.

## Deliberate spike debt

`dcomp-plane/src/main.rs` keeps D3D device construction, Windows creation, reporting, and test orchestration together. Splitting that disposable comparison into production-shaped services would manufacture abstractions for code the decision rejects. No production caller may depend on it.

The GPUI spike uses the private `SetWindowCompositionAttribute` ABI already used by the pinned GPUI Windows backend to remove an inherited accent tint. Production work must remove that local duplicate by extending the owned GPUI Windows adapter. The private seam must not leak into the effect core or Lua API.

The PowerShell GPU harness takes four bounded performance-counter samples solely to produce evidence. The manager, shell, renderer, and Lua hosts contain no such sampling loop.

## Required implementation checks

- Keep core effect transitions pure and renderer-neutral.
- Keep unsafe Windows calls in the GPUI Windows adapter with local safety proofs.
- Read shader bytes once from an opened immutable asset, digest and validate those same bytes, and submit that same buffer. Never check a path and reopen it.
- Preserve paths as `Path`/`OsStr` and platform names as native UTF-16/WTF-16.
- Deny `unwrap`, `expect`, and panic in non-test core code; propagate meaningful errors.
- Use one bounded channel per ownership crossing and one immutable scene plan per generation.
- Keep cancellation idempotent and safe at every await point.
- Run mixed-DPI, multi-monitor, HDR, device-removal, and sleep/resume gates before enabling those capability profiles.
