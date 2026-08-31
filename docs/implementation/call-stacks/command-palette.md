# First-party command palette

## Scope and decision

The command palette is a renderer-neutral shell capability with GPUI as its
first adapter. The shell owns query interpretation, result identity, ranking,
availability, argument requirements, and activation. GPUI owns focus,
selection, and presentation only.

Three designs were considered:

1. Let the GPUI view search catalog strings and invoke commands. This couples
   ranking and action validity to one renderer and would duplicate the same
   rules in WinUI or plugins.
2. Put every source into one generic string-result service. This erases the
   different authority and effect rules for manager actions, filesystem paths,
   content matches, applications, and web queries.
3. Keep one typed `CommandPalette` query plan with source-specific result and
   activation variants. Search adapters return stable domain identities and
   the palette performs one exhaustive activation match. This is selected.

The first vertical slice implements manager actions through the authorized
`CatalogSnapshot`. The selected design reserves `fff-search` for files and
content; action strings use `neo_frizbee`, the matcher used by `fff-search`,
without pretending actions are filesystem entries. The typed query parser
reserves the `!` web prefix. The next source slice installs its brokered
URL-launch adapter; the current UI does not advertise or accept an inert
placeholder mode.

Query parsing is a total typed transition:

```text
trimmed input
  -> empty                 -> PaletteQuery::Browse
  -> ordinary non-empty    -> PaletteQuery::Search(PaletteSearchTerms)
  -> `!` without terms     -> PaletteQuery::WebPrompt [not activatable]
  -> `!` with terms        -> PaletteQuery::WebSearch(WebSearchTerms)
```

The distinct non-empty term types prevent a local provider or URL broker from
accepting the other source's input by accident. Parsing selects authority; it
does not construct a URL or perform an effect.

`fff-search` 0.10.6 stores paths as UTF-8 strings created with
`to_string_lossy()` and reconstructs `PathBuf` values from those strings. Its
file result cannot be an execution operand on Windows until the adapter can
retain an opaque identity for the original `OsString`. Display text may be
lossy; activation paths must remain lossless WTF-16.

## Typed contract

```rust
pub struct CommandPalette { /* immutable projected actions and search index */ }
pub struct PaletteAction { /* presentation, availability, input contract */ }
pub struct PaletteMatches { /* ordered action identities and bounded cursor */ }

pub enum PaletteActionState<'a> {
    Ready(ActionBinding),
    RequiresInput(&'a [ActionParameter]),
    Unavailable(ActionUnavailability),
}

impl CommandPalette {
    pub fn project(catalog: &CatalogSnapshot) -> Self;
    pub fn actions(&self) -> &[PaletteAction];
    pub fn search(&self, query: &str) -> PaletteMatches;
}
```

The projected action owns bounded presentation text because renderer entities
outlive the catalog borrow. `PaletteActionState` is a runtime enum because
availability and parameter requirements come from the current manager state.
Only `Ready` can produce an `ActionBinding`; required arguments cannot be
silently omitted.

## Action search and activation stack

```text
GPUI input change: &str
  -> CommandPalette::search(&str) [hot-local, synchronous]
    -> neo_frizbee::match_list(search keys) [SIMD fuzzy rank]
      <- ordered borrowed PaletteAction values
  -> GPUI selection
    -> PaletteAction::state()
      -> Ready(ActionBinding)
        -> ShellHandle::invoke_binding(ActionBinding) -> InvocationTicket
          [bounded enqueue; caller cancellation does not cancel pipe I/O]
          -> shell session actor refreshes and binds current catalog
            -> authenticated command protocol
              -> manager admission
                -> native Windows effect
      -> RequiresInput(parameters) [no side effect]
      -> Unavailable(reason) [no side effect]
```

Transport, catalog refresh, lease allocation, and cancellation remain owned by
`ShellSession`. Search never performs I/O or holds a lock. A stale projected
action is rebound against the current catalog at activation and may be rejected
without retry.

## Future source stacks

```text
query parse
  -> default query -> actions + applications + files
  -> content mode -> fff-search grep adapter
  -> !query -> WebQuery (never interpreted as a shell command)

file selection: OpaquePathId
  -> lossless path registry lookup -> PathBuf / WindowsPathInput
    -> ShellExecuteExW adapter

web selection: WebQuery
  -> percent-encoded HTTPS search URL
    -> broker policy -> ShellExecuteExW adapter
```

File indexing and content search run in owned background tasks with explicit
startup, cancellation, and join. Query updates may supersede result interest,
but never cancel a filesystem operation at a point that corrupts an index.

## Proof obligations

- Public integration tests prove typo-tolerant action ranking, exact metadata,
  unavailable actions, and parameter-required selection.
- A real command protocol integration test proves palette activation reaches
  manager admission through `ShellHandle` without a second command path.
- The filesystem adapter must prove exact round trips for unpaired UTF-16
  surrogates before any FFF-derived result can be opened.
- The GPUI adapter must prove keyboard-only selection, dismissal, focus
  restoration, and one activation per Enter press.
