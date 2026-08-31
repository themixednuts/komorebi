# Renderer-neutral shortcut guide

## Scope and decision

The shortcuts surface is a projection of the manager's authorized action
catalog, not a second parser for `whkdrc` and not a renderer-owned command list.
Every configured native shortcut becomes a manager `BindingHint`; the shell
projects those hints with the matching action metadata, and GPUI renders the
owned result.

Three sources were considered:

1. Keep loading `whkdrc` in the shortcuts executable. This preserves an
   external command-string system and lets display state disagree with the
   manager's typed actions.
2. Let GPUI enumerate and interpret protocol definitions directly. This leaks
   protocol ordering, matching, availability, and filtering into the renderer.
3. Project the authorized `CatalogSnapshot` into a shell-owned
   `ShortcutGuide`. This makes configured bindings and action metadata one
   coherent snapshot and leaves GPUI as a presentation adapter. This is
   selected.

## Typed contract

```rust
pub struct ShortcutGuide { /* immutable owned entries */ }
pub struct ShortcutGuideEntry { /* trigger plus action presentation */ }

impl ShortcutGuide {
    pub fn project(catalog: &CatalogSnapshot) -> Self;
    pub fn entries(&self) -> &[ShortcutGuideEntry];
    pub fn search<'a>(&'a self, query: &str)
        -> impl Iterator<Item = &'a ShortcutGuideEntry>;
}
```

The protocol catalog already guarantees matched, sorted definitions and offers.
Projection emits one row per binding hint and retains `ActionAvailability`.
Rows own their bounded strings because a GPUI entity outlives the protocol
borrow used to construct it.

## Entrypoint to effect

```text
komorebi-shortcuts process startup
  -> ShellSession::start(OwnerControl, OneShot)
    -> ShellHandle::catalog_snapshot() -> CatalogTicket
      [bounded enqueue; dropping result interest never cancels pipe I/O]
      -> shell session actor refreshes authorized CatalogSnapshot
        -> ShortcutGuide::project(&CatalogSnapshot)
          [pure owned projection; no renderer or transport types]
          -> GPUI ShortcutGuideView
            [filter input changes local query; no I/O or polling]
            -> one row per matching binding hint
```

## Failure and ownership

- Negotiation and catalog failures stay typed until the executable reporting
  edge. GPUI never sees named-pipe errors.
- Session shutdown finishes the in-flight catalog read and joins its Tokio
  actor before the runtime is dropped.
- Empty binding hints produce an empty guide, not inferred or hardcoded keys.
- Search is synchronous and side-effect free; no keystroke triggers a command
  from this read-only guide.
- The old eframe and whkd parser dependencies are deleted in the same wave as
  the GPUI caller migration.

## Stable tests

- A real `CommandProtocolServer` proves catalog retrieval through the public
  owned session interface.
- A public `ShortcutGuide` integration test proves exact metadata projection,
  multiple triggers per action, availability retention, and filtering without
  renderer involvement.
