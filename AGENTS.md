# Project implementation rules

- Production work always uses the `tdd`, `call-stack-design`,
  `thermo-nuclear-code-quality-review`, and `rust` skills.
- Define the typed entrypoint-to-effect call stack and the stable test seam before
  implementing a vertical slice.
- Run one red test, implement only that behavior, then run the focused test green.
  Test behavior through public crate or process boundaries, not private helpers.
- Prefer Rust domain types, data-carrying enums, and typestate when callers know a
  capability state at compile time. Make invalid states unrepresentable where
  practical.
- Keep async at waiting boundaries. Each binary owns one Tokio runtime, every
  spawned task has an owner, and cancellation must leave durable and native state
  consistent.
- Preserve Windows paths losslessly as WTF-16 until a boundary explicitly proves
  UTF-8 is sufficient.
- Migrate every caller and delete the superseded API in the same implementation
  wave. Do not add compatibility shims, parallel implementations, fallback
  renderers, or `legacy` modules.
- End each slice with formatting, focused and workspace tests, strict Clippy, and
  a maintainability audit. Split files before a change pushes them past 1,000
  lines, and remove pass-through layers and scattered special cases.
- Precision Touchpad validation is deferred and must not block other production
  implementation.
