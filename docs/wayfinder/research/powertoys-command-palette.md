# PowerToys Command Palette integration boundary

## Decision

PowerToys Command Palette should be an optional, thin view adapter over the Windows manager's Rust-owned command catalog. It is a good system-wide entry point, but it must not own command identity, argument schemas, availability rules, settings, state, or execution. The first-party control surface and every other client must remain fully functional when PowerToys or the extension is absent.

Build the adapter as the officially supported C#/.NET out-of-process Command Palette extension. Give it one top-level **Windows Manager** list page, an optional fallback search result, and a deliberately small set of pinnable commands. It should obtain a versioned catalog snapshot and state/event stream from the manager, render those from an in-memory cache, and invoke commands through the same versioned local IPC used by the first-party surface. Do not embed Rust into the extension, shell out to `komorebic`, mirror configuration into extension settings, or create a second command model in C#.

This boundary gives the owner the PowerToys launcher they already use without turning PowerToys into a runtime dependency or constraining a future pivot away from komorebi compatibility.

## Evidence baseline

This research used Microsoft documentation and PowerToys source as of 2026-08-27. The PowerToys source observations refer to commit [`e5a19c4`](https://github.com/microsoft/PowerToys/tree/e5a19c4ac544b18d79da42895e7f5c116aee15cd). Command Palette is still explicitly marked preview, and Microsoft warns that APIs are incomplete and may break before 1.0. The same README describes the WinRT interface as language-agnostic but its toolkit as C#-specific. [PowerToys Command Palette README](https://github.com/microsoft/PowerToys/blob/e5a19c4ac544b18d79da42895e7f5c116aee15cd/src/modules/cmdpal/README.md#L201-L234)

The current public NuGet release is `Microsoft.CommandPalette.Extensions` 0.12.260812002, released 2026-08-13. The package remains below 1.0, and the published version history moved through 0.1, 0.2, 0.5, 0.6, 0.8, 0.9, and 0.12 in roughly seventeen months. [Microsoft.CommandPalette.Extensions on NuGet](https://www.nuget.org/packages/Microsoft.CommandPalette.Extensions/)

## What the extension can express

| Need | Current Command Palette expression | Boundary for the Windows manager |
| --- | --- | --- |
| Actions | Top-level commands, nested list items, fallback commands, and context-menu commands. Invocation results can dismiss, hide, stay open, navigate, show a toast, or request confirmation. [Extension model](https://learn.microsoft.com/en-us/windows/powertoys/command-palette/extensibility-overview) [current template reference](https://github.com/microsoft/PowerToys/blob/e5a19c4ac544b18d79da42895e7f5c116aee15cd/src/modules/cmdpal/ExtensionTemplate/TemplateCmdPalExtension/.github/instructions/cmdpal-extension.instructions.md) | Project catalog entries into list items. Keep stable `CommandId`, typed arguments, validation, availability, and execution results in Rust. Translate only the small subset of result presentation that Command Palette understands. |
| Searchable commands | `ListPage` provides host search; `DynamicListPage` receives search text and can replace its items. Fallback commands can react to the home-page query. `LoadMore` and `HasMoreItems` support incremental lists. [ListPage API](https://learn.microsoft.com/en-us/windows/powertoys/command-palette/microsoft-commandpalette-extensions-toolkit/listpage) [DynamicListPage API](https://learn.microsoft.com/en-us/windows/powertoys/command-palette/microsoft-commandpalette-extensions-toolkit/dynamiclistpage) | Return one cached command page, grouped by category and filtered by availability. Add one optional fallback item such as `Windows Manager: <query>`; do not register every catalog entry as a top-level provider command. |
| Searchable live state | A dynamic list can show windows, workspaces, layouts, rules, or other changing items. `RaiseItemsChanged` refreshes lists and property notifications refresh individual displayed values. The shipped extension guidance explicitly supports timer/event-driven updates. [current extension guidance](https://github.com/microsoft/PowerToys/blob/e5a19c4ac544b18d79da42895e7f5c116aee15cd/src/modules/cmdpal/ExtensionTemplate/TemplateCmdPalExtension/.github/instructions/cmdpal-extension.instructions.md#dynamic-updates) | Maintain a read-only local projection from a single manager subscription. Coalesce bursts and refresh at most once per rendered frame or short debounce interval. Never query manager topology synchronously from `GetItems()` or per keystroke. |
| Forms and arguments | `FormContent` renders Adaptive Card JSON, optionally binds data, and returns submitted values as JSON. It supports validation, confirmation, and mixed content. [Microsoft form documentation](https://learn.microsoft.com/en-us/windows/powertoys/command-palette/using-form-pages) | Use generated forms only for commands whose required arguments cannot be selected naturally from nested lists. Validate again in Rust. Do not treat Adaptive Cards as the catalog's canonical parameter schema or as the main configuration UI. |
| Settings | Extensions can expose a settings content page. The toolkit has text, toggle, and choice settings plus change notifications and JSON conversion. [Settings toolkit API](https://learn.microsoft.com/en-us/windows/powertoys/command-palette/microsoft-commandpalette-extensions-toolkit/settings) | Store only adapter preferences, such as which state result groups appear and whether a fallback command is enabled. Manager behavior and configuration remain manager-owned. Connection discovery should normally be automatic and local. |
| Icons and rich rows | Items support Fluent glyphs, emoji, package assets, SVG/remote images, executable resources, subtitles, tags, details, hero images, metadata links, sections, and grid layouts. [Microsoft extension samples](https://learn.microsoft.com/en-us/windows/powertoys/command-palette/samples) | Put semantic icon tokens in the Rust catalog and map them to Fluent glyphs or packaged local assets in C#. Avoid remote icon URLs for a local control path. Rich details may show command help, bindings, availability, and focused-window context. |
| Live results and feedback | Pages and providers can raise item/property changes; extensions can show inline progress, status messages, transient toasts, confirmations, and updated Dock labels. A shipped time/date example updates a Dock item every minute. [Dock extension documentation](https://learn.microsoft.com/en-us/windows/powertoys/command-palette/adding-dock-support) | Reflect cached manager events and command completion. For long operations, send the request asynchronously inside the adapter, show progress, then publish completion. Do not block the synchronous SDK callback waiting on manager work. |
| Dock commands | SDK 0.9 introduced `ICommandProvider3.GetDockBands`; `ICommandProvider4.GetCommandItem` resolves nested commands by stable ID. Bands can be one invokable button, a multi-button list strip, or a content flyout. [Dock extension documentation](https://learn.microsoft.com/en-us/windows/powertoys/command-palette/adding-dock-support) | Offer an opt-in, small band for stable high-frequency actions, such as command-surface activation, current layout, or focused-workspace actions. Do not recreate the manager AppBar in the PowerToys Dock. The Dock itself is another AppBar, has no auto-hide, and requires Command Palette to run. [Command Palette Dock behavior](https://learn.microsoft.com/en-us/windows/powertoys/command-palette/dock) |

## Packaging, process, and language constraints

Command Palette discovers extensions through the Windows Package Catalog. A standalone extension is an MSIX-packaged executable whose manifest registers both an out-of-process COM server and a `windows.appExtension` named `com.microsoft.commandpalette`; the same CLSID must be used by the C# implementation and both manifest registrations. Command Palette activates the process through WinRT/COM. [Microsoft extension architecture](https://learn.microsoft.com/en-us/windows/powertoys/command-palette/extensibility-overview) [current template manifest guidance](https://github.com/microsoft/PowerToys/blob/e5a19c4ac544b18d79da42895e7f5c116aee15cd/src/modules/cmdpal/ExtensionTemplate/TemplateCmdPalExtension/.github/instructions/cmdpal-extension.instructions.md#packageappxmanifest)

For private use, the package can be built and deployed locally. If it is distributed later, Microsoft documents Microsoft Store and WinGet packages, with the `windows-commandpalette-extension` WinGet tag providing palette discovery. [Publishing options](https://learn.microsoft.com/en-us/windows/powertoys/command-palette/publish-extension)

Each extension is a separate process. The generated program hosts an MTA COM server and waits until Command Palette disposes the extension. This is a valuable lifecycle and crash boundary from the Windows manager. [current COM server template](https://github.com/microsoft/PowerToys/blob/e5a19c4ac544b18d79da42895e7f5c116aee15cd/src/modules/cmdpal/ExtensionTemplate/TemplateCmdPalExtension/.github/instructions/cmdpal-extension.instructions.md#com-server-programcs)

The WinRT interface is intended to be language-agnostic, so a direct Rust implementation is theoretically possible if Rust supplies all WinRT interface and COM server machinery. The supported and maintained path is C# through `Microsoft.CommandPalette.Extensions.Toolkit`. The current source template targets .NET 10 on Windows, x64 and ARM64, enables MSIX tooling, and uses CsWinRT plus `Shmuelie.WinRTServer`. [current template project](https://github.com/microsoft/PowerToys/blob/e5a19c4ac544b18d79da42895e7f5c116aee15cd/src/modules/cmdpal/ExtensionTemplate/TemplateCmdPalExtension/TemplateCmdPalExtension/TemplateCmdPalExtension.csproj#L395-L477) C# is therefore the thinner and less risky adapter. Rust should remain on the other side of versioned local IPC.

## Latency and failure isolation

The SDK does not publish a latency SLA. Its visible interfaces are synchronous for item retrieval and command invocation, while dynamic updates are event-driven. Microsoft's own extension guidance warns that `GetItems()` is called frequently and that expensive work or logging does not belong there. [current extension guidance](https://github.com/microsoft/PowerToys/blob/e5a19c4ac544b18d79da42895e7f5c116aee15cd/src/modules/cmdpal/ExtensionTemplate/TemplateCmdPalExtension/.github/instructions/cmdpal-extension.instructions.md#common-mistakes)

The implementation activates an extension with `CoCreateInstance(..., CLSCTX_LOCAL_SERVER, ...)`, catches activation exceptions, and treats an RPC-server-not-running HRESULT as a stopped extension. It then calls `GetProvider` through the proxy without a documented timeout or cancellation boundary. [PowerToys `ExtensionWrapper`](https://github.com/microsoft/PowerToys/blob/e5a19c4ac544b18d79da42895e7f5c116aee15cd/src/modules/cmdpal/Microsoft.CmdPal.UI.ViewModels/Models/ExtensionWrapper.cs#L716-L924)

Consequences:

- An adapter crash does not crash or stop the Windows manager because they are separate processes and communicate over manager-owned IPC.
- Out-of-process COM does not guarantee that a slow or hung adapter cannot degrade Command Palette. All SDK callbacks must return from local memory quickly.
- Catalog and state discovery should occur on an adapter worker and update immutable snapshots. `GetItems()`, search updates, item property getters, and Dock refreshes should only read those snapshots.
- Manager requests need adapter-side deadlines, cancellation, and explicit disconnected results. A lost manager connection should yield one disabled/status row and retry with backoff, not repeatedly emit refresh events.
- Burst state events should be collapsed by generation. Stable object IDs should allow in-place property updates where possible; do not rebuild every command and Dock object on every window event.
- Command execution should be acknowledged quickly. Anything nontrivial should report pending/completed state rather than hold the Command Palette invocation call open.

This is deliberately one-way isolation: the PowerToys adapter is expendable; the manager is authoritative and independently operable. No manager process should load Command Palette SDK assemblies or depend on the extension package being installed.

## Preview churn policy

Treat the Command Palette SDK as a volatile edge dependency. Pin an exact NuGet version and its compatible PowerToys version in the adapter project, isolate every SDK type in that project, and test the package against the installed PowerToys build before updating either. Do not leak `ICommandItem`, Adaptive Card payloads, Dock interfaces, or WinRT types into Rust catalog/protocol crates.

Versioned interfaces such as `ICommandProvider3` and `ICommandProvider4`, the jump from SDK 0.9 to 0.12, and the main repository's explicit pre-1.0 warning are enough evidence to make a compatibility shim mandatory. The adapter should be replaceable without changing manager code. Feature detection should omit unavailable Dock capabilities rather than changing the catalog.

## Recommended adapter shape

```text
Rust manager
  command catalog + availability + argument schemas
  authoritative state + event stream
  versioned local request/response IPC
                 │
                 │ manager-owned protocol
                 ▼
C# CmdPal adapter process (optional MSIX)
  protocol client + reconnect/backoff
  immutable catalog/state cache
  SDK mapping + update coalescer
                 │
                 │ WinRT out-of-process COM
                 ▼
PowerToys Command Palette
```

The adapter needs only four internal responsibilities:

1. **Connect and cache:** negotiate protocol/capabilities, load a catalog snapshot, subscribe to state changes, reconnect with backoff, and atomically publish read-only projections.
2. **Project:** map catalog metadata to `ListItem`, details, context commands, forms, and optional Dock items without defining new behavior.
3. **Invoke:** send `{ command_id, typed_arguments, context_revision }`, translate explicit success/error/pending results, and never shell out.
4. **Throttle:** debounce search-backed remote queries, coalesce state generations, and keep every synchronous SDK callback local and bounded.

Initial surface:

- one top-level **Windows Manager** dynamic list page;
- sections for available commands, workspaces, windows, layouts, and help;
- one optional fallback result that searches the same cached projection;
- generated argument forms only where selection pages are insufficient;
- extension settings limited to visible result groups and fallback enablement;
- no PowerToys Dock band by default, with a small opt-in band after stable command IDs exist.

## Newly specifiable tickets

This research removes the external uncertainty needed to specify the following work:

1. **Define the catalog projection contract.** Specify stable command IDs, semantic icon tokens, categories, help/binding metadata, typed argument schemas, availability, context revisions, and explicit invocation results required by both first-party and Command Palette surfaces.
2. **Define versioned local catalog IPC.** Add catalog snapshot, capability negotiation, state/event subscription, request IDs, deadlines, cancellation, disconnected/error states, and generation numbers without coupling clients to manager implementation types.
3. **Build the optional C# Command Palette adapter.** Use the Microsoft template, exact SDK pinning, one dynamic root page, cache-only synchronous callbacks, generated forms, reconnect/backoff, update coalescing, and no `komorebic` process spawning.
4. **Add adapter contract and latency tests.** Run the same catalog/state fixtures used by the first-party surface; verify catalog parity, stable IDs, disconnected behavior, event bursts, stale generations, and bounded hot-path latency.
5. **Evaluate an opt-in Dock band after AppBar extraction.** Test simultaneous Shell AppBars, multi-monitor ordering, Explorer restart, fullscreen handling, and work-area reservation before exposing any manager Dock band. The PowerToys Dock is supplementary, never the manager's primary bar.

No further PowerToys research is required before tickets 1–4 are specified. Dock coexistence requires the separate native AppBar feasibility test, not more Command Palette API research.
