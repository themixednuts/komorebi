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
reserves the `!` web prefix. Web activation is admitted through one owned
broker and reaches the user's registered HTTPS handler through
`Windows.System.Launcher`; renderers never construct or launch a URI.

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
    Web(PaletteWebInvocation),
    File(PaletteFileInvocation),
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

pub trait WebUriLauncher {
    fn launch(
        &self,
        target: WebSearchTarget,
    ) -> impl Future<Output = Result<WebLaunchDisposition, WebLaunchFailure>> + Send;
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

## Web-search activation stack

The configured endpoint is parsed once at the composition boundary. It must be
HTTPS, contain a host, contain no credentials or fragment, and leave its one
validated query key unset. `WebSearchTarget` can only be constructed from that
endpoint and nonempty `WebSearchRequest`; URL encoding is owned by the `url`
crate rather than renderer string concatenation.

```text
GPUI Enter on `!terms` [direct foreground user action]
  -> PaletteController::activate() -> PaletteEffect::Web(PaletteWebInvocation)
    -> PaletteWebInvocation::submit(&WebSearchBroker)
      -> WebSearchBroker::Configured(WebActivationClient)
      -> acquire owned bounded-admission permit [async, cancellation-safe]
      -> bounded mpsc WebActivationCommand::Launch
      <- WebActivationTicket [broker now owns the effect]
        -> WebActivationService actor [single owner, ordered]
          -> WebSearchEndpoint::target(&WebSearchRequest)
            -> percent-encoded WebSearchTarget [HTTPS authority preserved]
          -> WebUriLauncher::launch(WebSearchTarget) [consumer-owned port]
            -> WindowsWebLauncher adapter
              -> Windows.Foundation.Uri
              -> Windows.System.Launcher::LaunchUriAsync [WinRT completion event]
              <- launched | rejected | translated HRESULT failure
        <- WebActivationTicket::complete
      <- PendingPaletteInvocation::complete -> PaletteCompletion
    -> PaletteController::complete [attempt fence]
      -> Applied | IgnoredStale
```

There is no timer or polling loop. Cancelling before broker admission produces
no effect. Once submission returns a ticket, dropping the GPUI task abandons
only result observation; the broker finishes the native attempt and releases
its permit. Shutdown closes admission, places a marker in its reserved channel
slot, closes the receiver at that marker, drains every command already admitted
behind it, and joins the task. A platform error is translated at
`WindowsWebLauncher`; WinRT types and HRESULT wrappers do not cross the port.

The broker does not query support before every launch. That would add a race and
a second native round trip without changing launch authority. A read-only native
test exercises `QueryUriSupportAsync` only as adapter evidence.

## Durable palette configuration stack

The web endpoint and exact file-search root are durable configuration, not JSON
state or compiled-in providers. `komorebi-settings` owns their typed Drizzle
schema, generated migrations, validation on load, and atomic singleton upserts.
The composition root reads them once; the configured brokers then own hot
in-memory projections for the process lifetime, so palette queries never touch
SQLite.

```text
komorebi-command-palette::main [Tokio composition root]
  -> %LOCALAPPDATA%/komorebi/settings.sqlite: PathBuf
    -> SettingsStore::open(&Path)
      -> komorebi_sqlite::open_durable
        -> rusqlite::Connection::open
        -> enable and verify WAL + synchronous=FULL + foreign_keys=ON
      -> Drizzle<SettingsSchema>::migrate [build.rs-generated migrations]
    -> SettingsStore::web_search
      -> typed Drizzle select -> WebSearchRow: SQLiteFromRow
      -> WebSearchEndpoint::new [authority validation]
        -> Some(endpoint)
          -> WebActivationService::start -> WebSearchBroker::Configured
        -> None
          -> WebSearchBroker::Unconfigured [explicit typed activation failure]
    -> SettingsStore::file_search_root
      -> typed Drizzle select -> FileSearchRow: SQLiteFromRow
      -> root_wtf16: BLOB -> exact PathBuf
        -> Some(root)
        -> None -> derive Windows home once -> typed Drizzle singleton upsert
```

`SettingsStore::set_web_search` and `set_file_search_root` use Drizzle's typed
insert/conflict/update query API; they do not interpolate raw SQL or maintain
manual migrations. The database is the durable source of truth and each broker
is a hot in-memory projection, so there is no cache-invalidation protocol.

The file root uses a raw BLOB column containing little-endian UTF-16 code units,
not JSON or JSONB. JSON strings cannot represent an unpaired surrogate, while a
Windows path may legally contain one. The codec rejects malformed odd-length
rows and reconstructs the original `OsString` without normalization. Other
platforms use the same BLOB column for their native path bytes.

SQLite internally passes even `sqlite3_open16` filenames through UTF-8. An
unpaired-WTF-16 database path therefore cannot preserve its filesystem identity.
The shared opener rejects such a path as `rusqlite::Error::InvalidPath`; it never
normalizes or silently opens a different file. Exact user file operands remain
opaque `PathBuf` values in the file-search stack and do not pass through SQLite.

## Remaining source stacks

```text
query parse
  -> default query -> actions + applications + files
  -> content mode -> fff-search grep adapter
  -> !query -> WebQuery (never interpreted as a shell command)

web selection: WebQuery
  -> percent-encoded HTTPS search URL
    -> broker policy -> Windows.System.Launcher::LaunchUriAsync adapter
```

File indexing and content search run in owned background tasks with explicit
startup, cancellation, and join. Query updates may supersede result interest,
but never cancel a filesystem operation at a point that corrupts an index.

## File-index stack

The file core is synchronous and immutable; `FileSearchService` owns it on one
long-lived Tokio blocking worker because `FilePicker` is not `Sync`. The
service does not enable FFF's watcher. Renderer tasks own only request/result
interest.

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

FileIndex::search_content(&ContentSearchTerms, ContentSearchLimit)
  -> fff_search::QueryParser
  -> FilePicker::grep(GrepMode::Fuzzy)
    -> exact retained PathBuf for every filesystem read
  -> validate result-local file index and one-based line number
  -> hard-truncate FFF's soft page bound
  <- ContentSearchMatch {
       id: OpaquePathId,
       display_path: String,
       line_number: NonZeroU64,
       byte_column,
       byte_offset,
       line_content,
     }

file activation
  -> FileIndex::resolve(&OpaquePathId)
    -> foreign/stale index identity -> None [no effect]
    -> matching identity -> exact PathBuf
      -> FileLauncher::launch(PathBuf) [consumer-owned port]
        -> WindowsFileLauncher
          -> UTF-16 with checked terminal NUL
          -> ShellExecuteExW adapter
```

`OpaquePathId` carries an exact path only for the bounded result page. The
index does not duplicate every path into a second registry. Rebuilding creates
a new identity, so results from the retired index cannot activate against its
replacement.

The async ownership stack is:

```text
FileSearchService::start(PathBuf, FileSearchQueueCapacity)
  -> tokio::task::spawn_blocking
    -> FileIndex::build
    -> readiness oneshot
  <- FileSearchService { client, owned JoinHandle }

FileSearchClient::search(query, limit)
  -> acquire owned bounded-admission permit [cancellation-safe]
  -> bounded Tokio mpsc Command::Search
    -> blocking worker owns FileIndex::search
      -> result oneshot
  <- Vec<FileSearchMatch>

FileSearchClient::resolve(OpaquePathId)
  -> same bounded admission
  -> blocking worker owns FileIndex::resolve
  <- Option<PathBuf>

FileSearchClient::search_content(terms, limit)
  -> same bounded admission and worker ownership
  -> FileIndex::search_content
  <- Vec<ContentSearchMatch> | typed invariant failure

FileSearchService::shutdown / Drop
  -> close admission semaphore
  -> reserved shutdown queue slot [no polling]
  -> worker drops receiver and index
  -> explicit shutdown awaits owned JoinHandle
```

The semaphore bounds queued plus executing requests; the Tokio channel has one
additional slot reserved for shutdown. Cancelling a caller either prevents
admission or drops its oneshot receiver. Once admitted, the worker completes
the read-only operation, releases the permit, and accepts later requests. The
service owner can therefore signal shutdown from `Drop` without a timer,
best-effort retry loop, or full-queue deadlock. Executables own the Tokio
runtime; this library neither creates one nor calls `block_on`.

The renderer-neutral integration adds a second fence distinct from activation
attempt identity:

```text
GPUI InputEvent::Change(raw query)
  -> PaletteController::update_query(&str)
    -> local action matches immediately [hot-local]
    -> nonempty local terms
      -> PaletteFileSearch { PaletteQueryRevision, owned terms }
  <- optional typed query effect + immediately renderable action rows
    -> PaletteFileSearch::submit(&PaletteFileSearchBroker)
      -> configured broker -> FileSearchClient::search [owned blocking worker]
      -> unconfigured broker -> typed Unavailable completion [no effect]
    <- PaletteFileSearchCompletion { revision, typed result }
      -> PaletteController::complete_file_search
        -> current loading revision -> apply bounded opaque file rows
        -> superseded revision -> IgnoredStale
  <- controller-borrowed presentation rows
```

`PaletteQueryRevision` and `PaletteAttemptId` are different domain identities:
query revisions fence replaceable read-only search results, while attempt IDs
fence non-repeatable activation outcomes. GPUI owns result interest only. It may
discard a superseded task, but it cannot cancel or corrupt work already admitted
to the index owner.

Activation uses a separate owned actor because native launch is non-repeatable:

```text
PaletteController::activate [selected row is a file]
  -> PaletteEffect::File(PaletteFileInvocation { attempt, opaque identity })
    -> PaletteFileInvocation::submit(&FileActivationClient)
      -> bounded actor admission
      <- FileActivationTicket [actor now owns the effect]
        -> FileSearchClient::resolve(OpaquePathId)
          -> None -> typed StaleIdentity [no native effect]
          -> exact PathBuf
            -> FileLauncher::launch(PathBuf)
              -> WindowsFileLauncher
                -> Tokio blocking pool [owned; no async-worker blocking]
                  -> ShellExecuteExW [native side effect]
        <- typed terminal result
      -> PaletteCompletion { attempt, result }
    -> PaletteController::complete [attempt fence]
```

Dropping the palette wait after ticket admission cannot cancel resolution or
split it from launch. The activation actor owns both steps and drains admitted
commands during shutdown. `WindowsFileLauncher` moves synchronous
`ShellExecuteExW` work to Tokio's blocking pool and awaits its owned join; a
cancelled UI future cannot detach or duplicate the native effect. The adapter
receives only the resolved exact path; presentation strings never cross this
boundary.

## Proof obligations

- Public integration tests prove typo-tolerant action ranking, exact metadata,
  unavailable actions, and parameter-required selection.
- A real command protocol integration test proves palette activation reaches
  manager admission through `ShellHandle` without a second command path.
- Controller tests prove bounded selection, duplicate-activation suppression,
  attempt uniqueness, source-specific web effects, and stale-completion
  rejection.
- Broker tests prove bounded admission, owned completion after UI interest is
  dropped, terminal shutdown, endpoint authority validation, percent encoding,
  and a registered native HTTPS handler without launching a browser.
- Settings tests prove generated migration application, absent configuration,
  durable typed upsert/reopen, endpoint validation, and rejection of an
  unrepresentable SQLite path without creating a different file identity.
- The filesystem adapter proves exact round trips for unpaired UTF-16
  surrogates in both the root and filename, including real file I/O through the
  resolved ID.
- The GPUI adapter must prove keyboard-only selection, dismissal, focus
  restoration, and one activation per Enter press.
