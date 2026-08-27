# Current Architecture and Extension Seams

## Research answer

Komorebi has one strong extension seam and two structural liabilities.

The strong seam is the serializable `SocketMessage` command vocabulary. The CLI, bar, GUI, and external tools already converge on it through `komorebi-client`, so it is the correct compatibility adapter for a shared command catalog. It is not yet a catalog: names, descriptions, parameter schemas, availability, bindings, and UI grouping live separately in Clap declarations, bar widgets, and `whkdrc`. The first architectural step should therefore be to introduce a transport-independent command model and adapt `SocketMessage` to it, rather than making a new UI enumerate `SocketMessage` directly. [`SocketMessage` is currently both the wire format and command vocabulary](../../../komorebi/src/core/mod.rs#L58-L277), while [`komorebic` manually maps its own subcommands to messages](../../../komorebic/src/main.rs#L2040-L3258).

The first liability is split state ownership. Monitor, workspace, container, and window topology lives in `WindowManager`, but rules, animation settings, visual settings, subscriptions, paths, and other policy live in process-wide mutexes and atomics. `State` and `GlobalState` then reconstruct two different projections of that state. Dynamic rules, Lua, and plugins should not be built on top of this split because they would acquire globals and mutate manager topology through unrelated paths. [`WindowManager` owns topology and a subset of policy](../../../komorebi/src/window_manager.rs#L72-L95); [the crate root owns many other mutable registries and settings](../../../komorebi/src/lib.rs#L81-L260); and [`State` and `GlobalState` expose the split](../../../komorebi/src/state.rs#L53-L155).

The second liability is that domain decisions and Windows effects are interleaved. A single locked `WindowManager` handles WinEvents and commands, calls Win32 through a broad `WindowsApi`, and directly triggers border, transparency, stackbar, and subscriber side effects. This gives the current manager a useful serialization point, but not an execution boundary suitable for scripts, isolated extensions, test doubles, or alternate UI runtimes. [Startup shares one `Arc<Mutex<WindowManager>>` among the command, event, and manager-notification loops](../../../komorebi/src/main.rs#L275-L347); [command handling performs effects and fan-out before returning](../../../komorebi/src/process_command.rs#L193-L214); [the tail of every command updates caches, subscribers, and visual managers](../../../komorebi/src/process_command.rs#L2307-L2327).

The uncommitted AppBar work improves the native integration boundary. It replaces a komorebi-specific work-area offset message with the Windows Shell AppBar contract and keeps Shell lifecycle handling in the bar process. That direction should be retained, then isolated behind a surface-host interface so egui and GPUI can be tested against the same AppBar behavior.

## Scope and evidence

This audit examined fork commit [`24c0ce0`](https://github.com/themixednuts/komorebi/tree/24c0ce0b1df1b5d7d62c682a7e28b32980293883) and the uncommitted AppBar working tree at `E:\Projects\komorebi` on 2026-08-27. The latter changed `komorebi-bar/Cargo.toml`, `komorebi-bar/src/bar.rs`, `komorebi-bar/src/main.rs`, and `komorebi/src/process_command.rs`, and added `komorebi-bar/src/appbar.rs`. Because that source was uncommitted at audit time, AppBar findings below identify the local file and symbols rather than claiming a durable repository link.

Only repository source and repository-owned documentation were used. The product language follows `E:\Projects\komorebi\CONTEXT.md`: compatibility is optional, personal continuity is required, and native integration means using documented Windows contracts.

## Runtime ownership map

```text
WinEvent hook ──> WindowManagerEvent channel ─┐
                                               ├─> Arc<Mutex<WindowManager>>
UDS/TCP JSON ──> SocketMessage ────────────────┘       │
                                                       ├─> monitor/workspace/container/window mutation
                                                       ├─> Win32 effects
                                                       ├─> global registries and atomics
                                                       └─> state snapshot notifications

bar / GUI / CLI ──> komorebi-client ──> socket
bar               <── subscriber socket <── Notification { event, State }
shortcuts          ──> reads whkdrc only
```

The event pump installs an out-of-context `SetWinEventHook`, converts selected Windows events into `WindowManagerEvent`, and sends them through a bounded channel. A dedicated event thread locks the manager and processes one event at a time. [The hook and channel are created in `winevent_listener`](../../../komorebi/src/winevent_listener.rs#L19-L67), [the callback performs event conversion](../../../komorebi/src/windows_callbacks.rs#L81-L149), and [the event listener acquires the manager lock](../../../komorebi/src/process_event.rs#L45-L91).

Command connections are accepted concurrently, but each command must acquire the same manager lock. UDS commands use a one-second `try_lock_for` and discard a command if the lock is unavailable; TCP commands wait on the lock. The UDS batch protocol is newline-delimited JSON, but query replies have no explicit response framing. [The UDS listener starts a thread per connection](../../../komorebi/src/process_command.rs#L103-L143), and [the command readers show the lock and framing behavior](../../../komorebi/src/process_command.rs#L2331-L2416).

The authoritative topology is nested mutable state:

- `WindowManager` owns a ring of physical monitors and the focused monitor.
- Each `Monitor` owns a ring of virtual workspaces and the focused workspace.
- Each `Workspace` owns tiled containers, floating windows, monocle/maximized state, layout policy, and a tiling/floating layer.
- Each `Container` owns one or more stacked windows.

The repository's [design document describes this nesting](../../../docs/design.md#L27-L43), and the [`Workspace` structure shows its current operational state](../../../komorebi/src/workspace.rs#L50-L97).

State outside this topology is extensive. Rule buckets, subscriber registries, monitor preferences, hiding behavior, titlebar behavior, current virtual desktop, layout defaults, animation configuration, and visual-manager configuration are globals. This is not merely configuration cache: commands mutate these values at runtime, and `GlobalState::default()` reads them back. [The registries are declared in `lib.rs`](../../../komorebi/src/lib.rs#L81-L260), and [`GlobalState::default()` samples them](../../../komorebi/src/state.rs#L157-L215).

`State` is a subscriber/query projection rather than the complete model. Its conversion intentionally strips `workspace_config`, while `GlobalState` must be queried separately. A future control surface, Lua host, or plugin API therefore cannot treat one current snapshot as complete authoritative state. [The stripping behavior is explicit in `From<&WindowManager> for State`](../../../komorebi/src/state.rs#L219-L245).

## Commands, configuration, and IPC

### Commands

`SocketMessage` is the most reusable existing seam because it is serializable, shared by clients, and covers both mutations and queries. Its current responsibilities are too broad:

- semantic window-manager commands;
- configuration mutations;
- queries and schema generation;
- subscription registration;
- transport-level stop and response behavior;
- low-level whole-state replacement.

All variants are dispatched by one large `WindowManager::process_command` match. The handler also checks virtual-desktop policy, snapshots state for change detection, writes ad hoc query replies, and fans out post-command notifications. [The enum mixes all command classes](../../../komorebi/src/core/mod.rs#L63-L263), and [`process_command` owns dispatch and response output](../../../komorebi/src/process_command.rs#L188-L215).

Command presentation is duplicated rather than described once. `komorebic` has its own Clap argument types and subcommand descriptions; the bar hardcodes messages inside widget behavior; `komorebi-gui` hardcodes messages from control callbacks; and `komorebi-shortcuts` knows only strings parsed from `whkdrc`. [Bar layout actions construct messages directly](../../../komorebi-bar/src/widgets/komorebi_layout.rs#L80-L139), [the GUI does the same](../../../komorebi-gui/src/main.rs#L245-L348), and [the shortcuts window displays raw binding commands](../../../komorebi-shortcuts/src/main.rs#L44-L77).

The command catalog should therefore be a new core abstraction with stable `CommandId`, typed arguments, descriptive metadata, context/availability predicates, and an executor result. Existing `SocketMessage` JSON becomes one adapter. Clap, PowerToys, first-party GPUI/egui surfaces, bindings, Lua, and extensions should all project from the catalog instead of defining command lists independently.

### Configuration

`StaticConfig` is a large Serde model that spans domain policy, visual settings, application rules, monitor/workspace setup, bar launch paths, and animation. Loading occurs in phases:

1. `preload` parses JSON, applies globals, creates the manager, and installs a file watcher.
2. manager initialization discovers monitors and windows.
3. `postload` applies monitor/workspace configuration and workspace rules.
4. file changes send `ReloadStaticConfiguration` through the same socket.

[The config surface is declared in `StaticConfig`](../../../komorebi/src/static_config.rs#L479-L676), [preload both mutates globals and constructs the manager](../../../komorebi/src/static_config.rs#L1265-L1375), and [postload mutates workspace topology and global workspace rules](../../../komorebi/src/static_config.rs#L1378-L1487).

This phased path is useful for personal continuity but is not an adequate scripting model. A Lua configuration should build a validated desired configuration and submit it through one reconciliation boundary. It should not call `apply_globals`, hold the manager lock, or mutate global rule vectors directly. Static JSON can remain an input adapter during the transition.

### IPC and subscriptions

`komorebi-client` is small at the socket layer, but it depends on and re-exports the full `komorebi` crate. Consequently, every UI or future extension that wants protocol types is coupled to manager internals, Windows-specific code, and the same type evolution. [The dependency is direct](../../../komorebi-client/Cargo.toml#L7-L10), and [the client re-exports manager domain and Windows types](../../../komorebi-client/src/lib.rs#L4-L84).

The default local transport uses Windows Unix-domain sockets. Optional TCP binds to `0.0.0.0`, and named pipes are supported for notifications. Subscribers receive `Notification { event, state }`; every delivered notification carries a complete `State` projection, optionally filtered when state did not change. [Client send/query/subscribe behavior is in `komorebi-client`](../../../komorebi-client/src/lib.rs#L97-L182), [TCP listening is externally reachable when enabled](../../../komorebi/src/process_command.rs#L146-L185), and [notification fan-out serializes a full snapshot](../../../komorebi/src/lib.rs#L348-L436).

Before Lua or isolated extensions, extract protocol/catalog types into a crate that does not depend on the manager implementation. Add a versioned request/response envelope, request IDs, explicit errors, capability discovery, and framed subscriptions. Out-of-process extensions should be preferred first because the current global state and direct Win32 effects make in-process failure isolation poor.

## Input, animation, and UI ownership

### Input

The current architecture intentionally delegates keyboard and mouse bindings to whkd or AutoHotkey, which launch `komorebic`; komorebi itself responds only to WinEvents and socket messages. [This is the documented design](../../../docs/design.md#L7-L24). The only first-party global mouse listener is `winput` for the custom focus-follows-mouse implementation, and it calls manager methods while taking the manager lock. [The movement listener shows that narrow use](../../../komorebi/src/process_movement.rs#L1-L41).

The bar owns local pointer gestures, but its `MouseMessage` can either send a `SocketMessage` or execute arbitrary PowerShell. This is a UI-local action model, not reusable input infrastructure. [Mouse actions are dispatched in bar config code](../../../komorebi-bar/src/config.rs#L470-L503).

First-party bindings, modal submaps, mouse actions, and touchpad gestures should normalize platform input into command-catalog invocations. The input host must not call `WindowManager` directly. This keeps whkd optional, supports binding introspection in the control surface, and ensures PowerToys, GPUI, scripts, and physical input use identical availability and validation rules.

### Animation

Animation currently supports only `Movement` and `Transparency`. Configuration is held in process-wide globals. `AnimationEngine` creates one thread per animation, controls cancellation through a global manager, sleeps to a target FPS, and delegates each frame to a renderer that applies Windows effects. [The available prefixes are explicit](../../../komorebi/src/animation/prefix.rs#L8-L20), [configuration is global](../../../komorebi/src/animation/mod.rs#L51-L80), and [the engine's execution model is thread-per-animation](../../../komorebi/src/animation/engine.rs#L56-L119).

This can animate independent HWND properties but does not represent semantic transitions such as “workspace A becomes B.” Workspace switching immediately restores the focused workspace and hides every other workspace, so a transition currently has no retained before/after scene to animate. [Monitor workspace loading is immediate hide/restore](../../../komorebi/src/monitor.rs#L170-L182), and [workspace hiding directly hides each window](../../../komorebi/src/workspace.rs#L379-L405).

Workspace animation and DWM-effect spikes need a transaction or scene-transition boundary: compute the next model state, retain source and destination window sets, ask a platform animation service to attempt the transition, then commit visibility/focus. The spike can determine which effects Windows permits, but the domain must stop assuming that topology mutation and immediate HWND movement are the same operation.

### Bar, shortcuts, and GUI

All three first-party surfaces use egui/eframe. The bar is the mature surface: it watches JSON config, subscribes to full manager state, keeps local view state, and instantiates widgets through a `BarWidget` trait. The trait is open internally, but `WidgetConfig` is a closed enum with a compile-time match, so it is not a runtime plugin seam. [Widget construction is centralized in the closed enum](../../../komorebi-bar/src/widgets/widget.rs#L35-L129), and [bar startup owns config watch, repaint, and subscription threads](../../../komorebi-bar/src/main.rs#L294-L395).

`komorebi-shortcuts` is not a command surface. It has no `komorebi-client` dependency, reads `whkdrc` once at startup, filters raw command strings, and cannot invoke, describe, validate, or discover commands. [Its dependencies contain only whkd parsing and eframe](../../../komorebi-shortcuts/Cargo.toml#L7-L13), and [its entire model is `Whkdrc` plus a string filter](../../../komorebi-shortcuts/src/main.rs#L5-L40).

`komorebi-gui` is a separate experimental control/configuration surface that queries state and sends hardcoded messages from egui callbacks. It does not supply a shared presentation or command model. [Its UI couples controls directly to IPC](../../../komorebi-gui/src/main.rs#L245-L348).

A GPUI comparison is therefore low-risk if it is built as another view adapter over a new UI-agnostic control-surface model. Reimplementing the existing bar state logic directly in GPUI would only transfer the coupling. The measured GPUI/GPUI Base/GPUI Component spike should share the same client, AppBar host, view-model inputs, and command catalog as an egui reference surface.

## Windows API boundary and live AppBar work

`WindowsApi` centralizes many operations: display enumeration, HWND positioning, focus, visibility, process inspection, DWM attributes, style changes, DPI, transparency, wallpaper, and hidden/border window creation. It is a static-method facade rather than an injected trait. [The facade begins with monitor discovery](../../../komorebi/src/windows_api.rs#L245-L416) and [continues through DWM, input, and wallpaper operations](../../../komorebi/src/windows_api.rs#L1034-L1468).

The boundary is incomplete. The WinEvent hook, borders, stackbars, monitor reconciliation, styles, COM virtual-desktop integration, and the bar import Windows APIs directly. HWNDs are stored as raw `isize` throughout the domain. A testable native-integration architecture should define narrow ports such as `WindowSystem`, `DisplaySystem`, `InputSystem`, `DesktopSystem`, `AnimationSystem`, and `ShellSurfaceHost`; keep the concrete Win32 implementation in platform modules; and prevent renderer crates from becoming alternate owners of system policy.

The live AppBar patch is directionally correct:

- `komorebi-bar/src/appbar.rs::AppBar` owns registration and unregisters in `Drop`.
- It subclasses the eframe HWND to receive the Shell callback message.
- It uses `ABM_NEW`, `ABM_QUERYPOS`, `ABM_SETPOS`, `ABM_REMOVE`, activation and position notifications, fullscreen z-order handling, and `TaskbarCreated` re-registration.
- `Komobar` owns `Option<AppBar>`, attaches after discovering its HWND, and releases registration on monitor disconnect.
- The old `MonitorWorkAreaOffset` path is removed from the bar, so Explorer owns the work-area reservation through the AppBar contract.
- A feature-gated Windows GUI subsystem removes the console without changing development builds.

Remaining coupling in that patch is worth addressing before a second renderer is added. `Komobar` discovers its process HWND by enumeration, stores monitor geometry in process-wide atomics, and constructs the AppBar inside egui application lifecycle code. Move those responsibilities into a `ShellSurfaceHost`/`AppBarHost` that accepts a real HWND, monitor identity, edge, and desired extent. Both egui and GPUI can then use exactly the same native contract, and AppBar correctness can be tested independently of rendering performance.

The three diagnostic `eprintln!` calls added to `komorebi/src/process_command.rs` are unrelated to AppBar ownership and should not enter the architecture branch.

## Coupling impact by planned capability

| Capability | Reusable seam | Blocking coupling | Architecture consequence |
| --- | --- | --- | --- |
| Shared command catalog | `SocketMessage`, `komorebi-client` | Commands have no metadata; Clap and UIs duplicate presentation; transport and semantics are one enum | Introduce catalog types and executor; retain `SocketMessage` as an adapter |
| Scratchpads | Workspace/container/window model; hiding primitives | A monitor has exactly one focused workspace, and loading it hides all others | Model a scratchpad as an overlay/special workspace with independent visibility and focus policy, not as a renamed normal workspace |
| Dynamic rules and tags | `MatchingRule`, regex cache, `should_act` | Identity matching maps into separate global behavior buckets; no tag/fact/action/lifecycle model | Create a state-owned rule engine with window facts, runtime tags, ordered actions, and reevaluation triggers |
| First-party input | `SocketMessage` invocation; narrow `winput` precedent | Input is delegated; custom FFM calls manager under lock; bindings are not discoverable commands | Add a platform input host that emits catalog invocations and owns binding/submap state |
| GPUI surfaces | IPC and subscriber stream; egui reference behavior | UI crates consume full manager types and duplicate state/action logic | Extract protocol and control-surface view models; compare renderers over identical adapters |
| Lua | Serializable commands and event notifications | Global mutable registries, no stable errors/capabilities, manager lock surrounds effects | Host Lua outside the lock and expose catalog/query/event APIs, not Rust internals |
| Isolated extensions | UDS/TCP/pipes and JSON serialization | Client depends on full manager crate; unversioned protocol; TCP has broad bind and weak framing | Define a versioned, capability-scoped out-of-process extension protocol first |
| Workspace animation | Movement/transparency dispatchers | Workspace switch immediately hides/restores HWNDs; no semantic transition state | Add transition transactions and inject an animation platform port before DWM spikes |
| AppBar | Dedicated bar process and native Shell contract in live patch | HWND discovery, monitor atomics, and egui lifecycle own native registration | Extract a renderer-neutral `AppBarHost`; preserve the Shell-owned reservation model |

The existing matching machinery is reusable but narrower than a dynamic rule engine. `MatchingRule` supports simple and composite identity rules over title, executable, class, and path with equality, substring, regex, and negative strategies. [`MatchingRule` and strategies are data types](../../../komorebi/src/core/config_generation.rs#L57-L115), while [`Window::should_manage` reads behavior-specific global buckets](../../../komorebi/src/window.rs#L789-L864) and [`should_act` evaluates identity only](../../../komorebi/src/window.rs#L1054-L1108). Runtime tags should be window-manager state, not another global matcher list.

Scratchpads likewise require a domain addition, not a command alias. Today each monitor's workspace ring has one focus index, and `load_focused_workspace` hides every other workspace. The overlay needs to coexist visibly with the normal focused workspace, retain its own last-focused window and geometry, and define monitor-follow, summon, dismiss, focus-return, and application-rule behavior.

## Recommended dependency order

1. **Extract the command/protocol core.** Define command identity, typed arguments, metadata, validation, availability, results, versioned IPC envelopes, and event envelopes. Adapt existing `SocketMessage`, CLI, and client without preserving internal compatibility at the cost of the new model.
2. **Consolidate runtime state and effect ports.** Move mutable policy registries under an explicit manager-owned state/configuration object. Separate transition decisions from `WindowSystem`/`ShellSurfaceHost` effects.
3. **Specify and build scratchpads.** The domain and command prerequisites will then be explicit, and no DWM assumption is needed.
4. **Specify and build dynamic rules/tags.** Build ordered facts, tags, actions, and reevaluation on the consolidated state model.
5. **Build the command-backed control surface and input host.** Keep PowerToys as an optional catalog adapter. Replace `komorebi-shortcuts` with a first-party surface able to discover and invoke the same catalog. Add keyboard/submap support, then mouse and touchpad adapters.
6. **Run paired egui/GPUI spikes.** Use the same view model, AppBar host, command catalog, test data, and performance protocol. GPUI adoption remains an evidence-based renderer decision.
7. **Add semantic transition boundaries and run Windows feasibility spikes.** Measure workspace animations and DWM effects only after the source/destination scene lifecycle is representable.
8. **Add Lua, then isolated extensions.** Both consume the catalog, query model, and event stream. Start out of process for extensions; consider in-process plugins only if measured latency requires them and a crash boundary is acceptable.

## Newly specifiable tickets

This audit resolves enough architecture uncertainty to specify the following implementation or design tickets without further repository research:

1. **Define the command catalog and protocol crate.** Acceptance can require metadata parity for existing interactive `SocketMessage` variants, typed argument schemas, explicit results/errors, request IDs, and adapters for the current socket JSON and Clap CLI.
2. **Define manager-owned runtime state and native-effect ports.** Acceptance can enumerate every current global registry and require each to become immutable process context, manager-owned state, or a clearly owned service.
3. **Specify the scratchpad domain contract.** Acceptance can cover overlay visibility, monitor targeting, summon/dismiss idempotence, focus restoration, floating geometry, restart state, and rule-based assignment.
4. **Specify runtime window facts, tags, and ordered rules.** Acceptance can preserve current identity matching while adding lifecycle triggers, runtime mutation, conflict ordering, inspection, and deterministic reevaluation.
5. **Specify first-party input over catalog invocations.** Acceptance can cover global bindings, modal submaps, conflict reporting, binding discovery, whkd coexistence, and input-thread isolation from the manager lock.
6. **Specify the shared control-surface model and paired egui/GPUI spike.** Acceptance can require identical commands and state fixtures plus startup, first-frame, idle CPU, memory, resizing, DPI, accessibility, AppBar, and implementation-complexity measurements.
7. **Harden and extract the AppBar host.** Acceptance can cover Explorer restart, fullscreen applications, monitor disconnect/reconnect, DPI and geometry changes, multiple bars, cleanup, and renderer parity.
8. **Specify semantic workspace transitions and DWM feasibility measurements.** Acceptance can separate domain transition correctness from optional native visual effects and record unsupported behavior from experiments rather than assumptions.
9. **Specify Lua and isolated extension contracts after tickets 1–2.** Both should be catalog/query/event consumers with explicit capability and failure boundaries; neither should receive mutable access to manager internals.

No additional architecture discovery is required before the first seven tickets are written. Lua and extension implementation details should wait for the command/protocol and state-ownership contracts, while DWM effect scope should wait for measured feasibility spikes.
