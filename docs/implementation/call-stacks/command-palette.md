# First-party command palette

## Scope and decision

The command palette is a renderer-neutral shell capability with GPUI as its
first adapter. The shell owns query interpretation, result identity, ranking,
availability, argument requirements, selection, activation state, and stale
completion fencing. GPUI owns physical input translation, focus, windowing,
and presentation only.

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

Upstream `fff-search` 0.10.7-nightly.f3647b7 stores paths as UTF-8 strings
created with `to_string_lossy()` and reconstructs `PathBuf` values from those
strings. The pinned source fork retains the exact `PathBuf` only when a Windows
path cannot round-trip through UTF-8. Its lossy string remains a search and
display projection. `komorebi-search` then seals the operand inside an opaque
identity tied to one immutable index instance; no display string can become a
file operand.

## Typed contract

```rust
pub struct CommandPalette { /* immutable projected actions and search index */ }
pub struct PaletteController { /* results, selection, status, attempt sequence */ }
pub struct PaletteAction { /* presentation, availability, input contract */ }
pub struct PaletteMatches { /* ordered action identities and bounded cursor */ }

pub enum PaletteEffect {
    Invoke(PaletteInvocation),
}

pub enum PaletteSubmission {
    Pending(PendingPaletteInvocation),
    Complete(PaletteCompletion),
}

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

`PaletteController` is the sole mutable owner. Renderers borrow visible rows
and status from it and submit the typed effect it returns. `PaletteAttemptId`
is a non-zero 128-bit sequence value. A completion changes state only while its
attempt is the one currently submitting; delayed or duplicated completions are
reported as `IgnoredStale`.

## Action search and activation stack

```text
GPUI InputEvent::Change: &str
  -> PaletteController::update_query(&str) [hot-local, synchronous]
    -> PaletteQuery::parse(&str)
      -> CommandPalette::query(PaletteQuery)
        -> neo_frizbee action rank [SIMD, hot-local]
  <- controller-owned rows, bounded selection, and PaletteStatus

GPUI Enter/click
  -> PaletteController::activate() -> Option<PaletteEffect>
    -> PaletteAction::state()
      -> RequiresInput(parameters) -> PaletteStatus::RequiresInput [no effect]
      -> Unavailable(reason) -> PaletteStatus::Unavailable [no effect]
      -> Ready(ActionBinding)
        -> PaletteInvocation { PaletteAttemptId, ActionBinding }
          -> PaletteInvocation::submit(&ShellHandle) -> PaletteSubmission
            -> immediate queue failure -> PaletteCompletion { result: Err(_) }
            -> InvocationTicket -> PendingPaletteInvocation [owned GPUI task]
              -> ShellSession actor [bounded queue; caller cancellation-safe]
                -> refresh and bind current catalog
                  -> authenticated command protocol
                    -> manager admission
                      -> native Windows effect
              <- accepted | retained | rejected | typed execution failure
            <- PaletteCompletion { attempt, typed result }
          -> PaletteController::complete(PaletteCompletion)
            -> Applied | IgnoredStale
  <- borrowed PaletteStatus for presentation
```

Transport, catalog refresh, lease allocation, and cancellation remain owned by
`ShellSession`. Search never performs I/O or holds a lock. A stale projected
action is rebound against the current catalog at activation and may be rejected
without retry. Dropping the GPUI wait task drops only result interest; the
session actor still owns the admitted operation and converges it independently.
The view retains at most one pending task because the controller suppresses a
second activation while one attempt is submitting.

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

## File-index stack

The first file slice is deliberately synchronous and immutable. It establishes
the lossless identity boundary before adding a long-lived task owner. It does
not enable FFF's watcher. The async provider slice will own index construction
and replacement on one dedicated blocking worker because `FilePicker` is not
`Sync`; renderer tasks will own only request/result interest.

```text
FileIndex::build(PathBuf) [blocking worker boundary]
  -> fff_search::FilePicker::new(FilePickerOptions { watch: false, ... })
    -> Windows canonicalization [PathBuf remains exact WTF-16]
  -> FilePicker::collect_files()
    -> walker FileItem creation
      -> UTF-8 search projection
      -> non-UTF-8 Windows path -> retain exact PathBuf
  <- immutable FileIndex { picker, unforgeable index identity }

FileIndex::search(&str, FileSearchLimit)
  -> fff_search::QueryParser
  -> FilePicker::fuzzy_search(... PaginationArgs)
  <- FileSearchMatch {
       display_path: String,       [presentation only]
       id: OpaquePathId {
         owner: index identity,
         exact_path: PathBuf,      [private operand]
       },
     }

file activation
  -> FileIndex::resolve(&OpaquePathId)
    -> foreign/stale index identity -> None [no effect]
    -> matching identity -> &Path
      -> future WindowsPathInput boundary
        -> ShellExecuteExW adapter
```

`OpaquePathId` carries an exact path only for the bounded result page. The
index does not duplicate every path into a second registry. Rebuilding creates
a new identity, so results from the retired index cannot activate against its
replacement.

## Proof obligations

- Public integration tests prove typo-tolerant action ranking, exact metadata,
  unavailable actions, and parameter-required selection.
- A real command protocol integration test proves palette activation reaches
  manager admission through `ShellHandle` without a second command path.
- Controller tests prove bounded selection, duplicate-activation suppression,
  attempt uniqueness, and stale-completion rejection.
- The filesystem adapter proves exact round trips for unpaired UTF-16
  surrogates in both the root and filename, including real file I/O through the
  resolved ID.
- The GPUI adapter must prove keyboard-only selection, dismissal, focus
  restoration, and one activation per Enter press.
