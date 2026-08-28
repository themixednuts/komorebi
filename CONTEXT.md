# Windows Manager

This context names the product concepts used while evolving the fork from a compatible komorebi variant toward a Windows manager with its own identity.

## Language

**Windows manager**:
The product that arranges windows and owns the surrounding workspace experience on Windows. It may begin as a komorebi fork without remaining defined by upstream compatibility.
_Avoid_: komorebi clone, Hyprland clone

**Compatibility**:
The ability to keep using existing komorebi commands, configuration, and integrations. Compatibility is useful until it conflicts with a cleaner Windows manager design.
_Avoid_: parity, hard compatibility requirement

**Personal continuity**:
The guarantee that observable window-manager behavior and Windows appearance remain intact while the manager changes. Internal formats and process boundaries may change when migration carries the active setup forward.
_Avoid_: public backward compatibility, release compatibility

**Active installation**:
The manager version and configuration that Windows launches for the owner. Source trees and successful builds are not active installations.
_Avoid_: latest build, current branch

**Startup reference**:
An OS-owned pointer that selects the exact active executable and arguments at login. A running process does not prove that the next login uses the same reference.
_Avoid_: startup script, current process

**Staged build**:
A candidate manager version prepared and validated without replacing the active installation.
_Avoid_: update, deployed build

**Promotion**:
The owner's explicit act of making a staged build the active installation through a managed restart.
_Avoid_: automatic update, build completion

**Promotion transaction**:
The recoverable record of an in-progress promotion that determines whether startup completes the switch or restores the known-good installation.
_Avoid_: copy operation, restart script

**Promotion health**:
The observable conditions a staged build must satisfy before and immediately after promotion to protect personal continuity.
_Avoid_: successful build, process is running

**Known-good installation**:
An active installation whose promotion health was verified on the owner's machine and which remains eligible for rollback.
_Avoid_: previous build, latest release

**Rollback**:
A single automatic return from a failed promotion to the most recent known-good installation.
_Avoid_: retry loop, downgrade

**Safe stop**:
The recovery state that removes the manager's Windows-side effects and leaves Explorer usable when neither promotion nor rollback is healthy.
_Avoid_: crash loop, factory reset

**Manager-owned shell layer**:
The accessible AppBar, palette, overview, notification history or proved presenter, quick controls, and optional OSD surfaces owned by the Windows manager while Explorer remains available for recovery.
_Avoid_: DWM replacement, Explorer replacement

**Personal shell profile**:
The selection of manager-owned features, consent-gated modes, and Explorer recovery routes enabled for the active installation. It keeps Explorer available unless a separate disposable shell-replacement experiment explicitly removes it.
_Avoid_: shell replacement, product defaults

**Notification presenter**:
The single component allowed to display notification popups in the active profile. Notification observation and history do not grant a second component permission to display the same notification.
_Avoid_: notification mirror, dual notifications

**Doctor**:
A read-only assessment of the active installation, its references, and promotion health.
_Avoid_: repair command, automatic fix

**Repair**:
An explicit restoration of generated manager state that does not reinterpret or reset user configuration.
_Avoid_: reset, config normalization

**Durable asset**:
Appearance content required by the active setup whose lifetime is independent of source trees and build outputs.
_Avoid_: build output, workspace asset

**Runtime state**:
Disposable process-owned data required while the manager runs, such as socket markers and the window recovery cache. The manager can recreate it without changing configuration or durable assets.
_Avoid_: user data, installation content

**Native integration**:
Behavior implemented through documented Windows contracts so that Windows and other applications recognize it correctly.
_Avoid_: native-looking, skin

**Command catalog**:
The single searchable set of window-management actions exposed through first-party controls and external launchers.
_Avoid_: shortcuts list, duplicate command menus

**Binding**:
A stable mapping from a normalized physical trigger and foreground context in one binding mode to a typed action or mode transition.
_Avoid_: hotkey command, shell shortcut

**Binding mode**:
A named input context in the manager's hierarchical binding map. Entering a child mode changes which bindings own input until the mode exits, expires, or is cancelled.
_Avoid_: submap process, key layer

**Input authority**:
The one active component allowed to interpret global keyboard and mouse input as manager bindings. External tools may invoke manager actions, but they do not share capture authority with first-party input.
_Avoid_: hook priority, input compatibility

**Input suspension**:
A temporary first-party pass-through state that clears active modes, held interactions, and repeats while retaining a protected route to resume capture.
_Avoid_: manager pause, input cancellation

**Workspace**:
An ordered manager-owned arrangement of application windows on one monitor. Exactly one ordinary workspace is active per monitor, and a scratchpad does not replace it.
_Avoid_: virtual desktop, desktop

**Windows desktop visibility domain**:
The external Task View grouping that Windows applies to top-level windows. The manager observes membership and visibility without treating Windows desktop identifiers, names, or ordering as manager workspaces.
_Avoid_: workspace, manager desktop

**Desktop observation wake**:
A filtered native notification that tells the manager to re-observe the Windows desktop visibility domain without claiming what changed. The primary Windows 11 wake is the desktop window's accessibility-name change; managed-window cloak changes are corroborating wakes.
_Avoid_: desktop-changed fact, polling tick

**Desktop settlement burst**:
A bounded sequence of public per-window observations started by a desktop observation wake and stopped after three equal cohort snapshots. Candidate or unavailable evidence cannot change window visibility.
_Avoid_: background polling, desktop debounce timer

**Container**:
One ordered layout position in a workspace that holds one or more application windows. A multi-window container exposes one active member at a time without becoming a new workspace.
_Avoid_: tile window, tab group

**Stack**:
A container with two or more ordered window members sharing one layout position. Its stackbar presents the order and active member but does not own them.
_Avoid_: window list, tabbed window

**Container lock**:
Explicit protection that prevents a container from being moved or having its membership or order changed. Other containers may be placed beside it and workspace relayout may change its geometry, but unlocking is never implicit.
_Avoid_: fixed rectangle, drop guard

**Placement session**:
One generation-fenced attempt to move a stable window identity from its committed source toward an exact structural target. Previewing or cancelling a placement session does not change manager intent.
_Avoid_: drag state, pending move

**Placement target**:
A revision-bound semantic destination that names an exact container insertion index, split side, or empty workspace position. Screen coordinates may discover a target but are not the target itself.
_Avoid_: drop rectangle, cursor position

**Window tag**:
A stable manager-owned label asserted on a window with recorded provenance and lifetime. Tags describe membership or intent without mutating the application window.
_Avoid_: application label, window property

**Window rule**:
A named declarative policy that contributes tags or manager intent while its typed predicate and lifecycle apply. A window rule cannot execute arbitrary commands or call native APIs.
_Avoid_: callback, command script

**Window policy explanation**:
The revisioned account of observed facts, matching rules, conflicts, suppressed contributions, and winning manager intent for one window.
_Avoid_: debug log, rule trace

**Window family**:
One foreign top-level root and the currently observed owned windows that must retain their application-defined ownership and focus relationships. Membership follows window identity and owner relationships, not process identity.
_Avoid_: process window group, reparented window tree

**Surface role**:
The manager's revisioned classification of one foreign window within its window family, such as primary window, dialog, utility window, menu, tooltip, or unknown transient surface.
_Avoid_: window type, style guess

**Modal constraint**:
A known or unresolved condition in a window family that may redirect or block interaction through a dialog. While it remains active or uncertain, the manager does not hide, move away, or bypass the affected family.
_Avoid_: modal flag, blocked window

**Placement coordination**:
A manager request that preserves a foreign window's size, ownership, styles, and activation behavior while recovering or applying an explicitly configured position.
_Avoid_: popup management, window takeover

**Scratchpad**:
A named manager-owned collection of application windows that can be presented above an ordinary workspace on one monitor or held hidden without changing that workspace.
_Avoid_: special workspace, hidden workspace

**Scratchpad presentation**:
The one monitor attachment through which a scratchpad is shown, including its layout, placement, focus, and z-order intent. A scratchpad has at most one presentation.
_Avoid_: scratchpad workspace, overlay window

**Action**:
A fully typed request to change manager-owned state or behavior. Queries, search results, transport controls, and arbitrary command strings are not actions.
_Avoid_: socket message, palette result

**Action definition**:
The stable discoverable identity and meaning of one action, including its parameters, description, permitted uses, confirmation policy, and undo policy. It contains no caller-specific or live state.
_Avoid_: CLI subcommand, wire schema

**Action offer**:
A revisioned view of an action definition for one caller, enriched with current availability, current value, dynamic parameter choices, and active bindings.
_Avoid_: static descriptor, action result

**Action invocation**:
One identified attempt to execute a fully bound action against an expected manager revision and under resolved caller authority.
_Avoid_: key press, socket write

**Control protocol**:
The versioned local contract through which authenticated clients discover catalog entries, observe authorized manager views, subscribe to committed events, and invoke actions. It exposes stable domain values rather than manager, transport, or UI implementation types.
_Avoid_: SocketMessage API, IPC schema

**Control principal**:
The identity and authority the manager assigns to one authenticated control connection from operating-system and launch evidence. A client-declared name, role, or grant cannot create it.
_Avoid_: client ID, claimed role

**Manager epoch**:
The identity of one lifetime of authoritative manager state. State revisions and event positions are comparable only inside the same manager epoch.
_Avoid_: process ID, boot number

**Event cursor**:
A manager epoch and committed-event position that identifies subscription progress. It supports bounded resume and makes a missing delivery require an explicit snapshot resynchronization.
_Avoid_: last notification, socket offset

**Control surface**:
The first-party interface for finding, understanding, and invoking entries from the command catalog.
_Avoid_: shortcuts window, general-purpose launcher

**Toolkit projection**:
A renderer-specific presentation of a renderer-neutral shell snapshot. It may translate raw UI input and publish pixels and accessibility state, but it cannot own catalog identity, palette-session truth, manager intent, or native effects.
_Avoid_: UI state owner, toolkit domain model

**Surface renderer assignment**:
The one toolkit projection allowed to present and accept input for a manager-owned shell feature in an active installation. A staged candidate may be compared with it, but cannot register Windows effects or become a second live presenter.
_Avoid_: dual renderer, fallback renderer

**Command palette**:
The first-party searchable shell window that combines command-catalog actions with separately typed application, file, file-content, and explicit web search sources. Search results do not become manager commands.
_Avoid_: shortcuts window, PowerToys adapter

**Search source**:
A typed source of command-palette results with its own query, privacy, ranking, and activation rules.
_Avoid_: command type, untyped result list

**Palette session**:
One opening of the command palette, bound to the manager context captured before the palette takes focus and to a monotonic sequence of query generations.
_Avoid_: palette process, search window

**Search result**:
A source-owned candidate identified by its stable item identity and the palette, query, and source generations that produced it. It is not an action.
_Avoid_: result row, command

**Result activation**:
One identified attempt to act on an exact selected search result after its identity, source generation, availability, authority, and captured context are revalidated.
_Avoid_: Enter key, result callback

**Search index**:
A generation-stamped local view of permitted file names and file content whose building, refreshing, stale, and unavailable states remain explicit.
_Avoid_: file cache, search database

**Configuration profile**:
A durable owner-authored declaration of manager policy, bindings, shell choices, and automations compiled as one revision from one selected source plus explicit manager-owned edits. Live window state and generated recovery data are not part of it.
_Avoid_: static configuration, active installation

**Automation**:
A named response to committed manager events or manager-owned timers that may request typed actions under an explicit capability grant. It never runs inside a manager transition or native effect.
_Avoid_: window rule, callback command, startup script

**Extension package**:
An immutable, explicitly installed bundle of non-executable manifest data and text Lua modules that runs under its own identity, capability grant, generation, and isolated Rust host process. Installation does not grant authority or make the package trusted.
_Avoid_: plugin DLL, loose script folder

**Extension principal**:
The manager-resolved identity that binds one exact extension package, active generation, and grant revision to every event, request, contribution, stored value, and diagnostic it produces.
_Avoid_: script name, self-declared plugin identity

**Extension contribution**:
A bounded, declarative, toolkit-independent item offered by an extension package to manager-owned control surfaces, such as a namespaced action, status item, settings form, or palette search source. It contains no renderer callback or native handle.
_Avoid_: widget plugin, UI callback

**Feasibility spike**:
A disposable, measured experiment that establishes what Windows permits and where the practical limits lie before a feature is designed.
_Avoid_: assumption, production prototype

**Authoritative state**:
The sole manager-owned record from which window-management decisions and published views derive. It contains committed intent, relevant platform observations, policy, and lifecycle state.
_Avoid_: global state, state snapshot

**Manager intent**:
The arrangement or policy the manager has committed to pursue. It does not assert that Windows has already applied the corresponding effects.
_Avoid_: actual window state, requested command

**Platform observation**:
A fact reported or measured from Windows about externally owned state, such as window existence, foreground identity, or geometry.
_Avoid_: manager event, desired state

**Transition**:
One ordered and atomic change to authoritative state caused by an accepted input. A transition records its revision and the effects and facts it produces.
_Avoid_: command handler, Win32 operation

**Motion presentation**:
A temporary visual account of an already committed transition. It is never authoritative and may be skipped or cut short without changing manager intent.
_Avoid_: transition, animated state

**Presentation cover**:
The first complete, privacy-safe visual frame that prevents partial native placement from becoming visible while a motion presentation begins.
_Avoid_: screenshot, loading frame

**Visual settlement**:
The end of a motion presentation, when its temporary visuals are gone and any remaining mismatch with manager intent is an explicit reconciliation or degradation concern.
_Avoid_: animation finished, native success

**Effect plan**:
The ordered native work derived from a transition, with each effect classified by its required, convergent, or best-effort behavior.
_Avoid_: transaction, side-effect call

**Effect outcome**:
The typed report that Windows applied, rejected, timed out, or left uncertain one planned native effect. It returns through the ordered input path and does not claim that the resulting platform state has been observed.
_Avoid_: success flag, final window state

**Unavailable observation**:
A typed report that Windows could not currently supply a requested platform fact. It preserves existing intent and must not be treated as destruction, absence, or a synthetic default value.
_Avoid_: destroyed window, zero frame

**Committed event**:
A revisioned domain fact published after its transition commits, identified by the input that caused it.
_Avoid_: socket message, notification request

**Reconciliation**:
A transition that compares manager intent with fresh platform observations and chooses how to converge or report degradation.
_Avoid_: rollback, retry loop

**Compensation**:
An attempt to restore exact captured state after a native effect only where that state can be observed and restored reliably.
_Avoid_: synthetic default, guaranteed rollback

**Derived effect owner**:
A component that maintains a manager-owned surface or applies native work from committed intent without independently changing application-window truth.
_Avoid_: secondary state owner, direct window controller

**Preview generation**:
The validity epoch shared by a window preview and its source observations. Pixels from an earlier generation are not valid content for a later generation.
_Avoid_: frame age, cache version

**Preview placeholder**:
A manager-owned representation shown when a window has no current safe preview. It reveals only the identity and reason allowed by the window's privacy policy.
_Avoid_: stale preview, capture fallback image

**Visual mutation record**:
The minimal durable proof of a reversible manager-owned change to a foreign window. It identifies the exact window instance, its known prior state, and the manager's observed change without storing window content or titles.
_Avoid_: window snapshot, style default

**Visual mutation ownership**:
The temporary authority to vary a foreign window while its exact prior state remains restorable. Ownership ends only after restoration is observed or the window instance ceases to exist.
_Avoid_: opacity toggle, style override

**Visual safety degradation**:
A reported operating state that disables new foreign-window appearance changes after exact restoration cannot be proved. Core window management continues without claiming that uncertain application state was repaired.
_Avoid_: visual rollback, forced reset

**Input transition**:
An ordered keyboard or button state change that the input service must deliver without loss.
_Avoid_: pointer sample, repeatable input

**Pointer sample**:
The newest physical pointer observation available to the manager. A newer sample may replace an older pending sample without delaying input transitions.
_Avoid_: lossless mouse event, input transition

**Input generation**:
A validity epoch for queued input. Session, desktop, device, and broker boundaries advance it so work from an earlier epoch cannot execute.
_Avoid_: timestamp, retry count

**Privileged broker**:
A narrow high-integrity helper for specific native window operations that Windows denies to the medium-integrity manager. It does not own input, configuration, scripting, UI, or network access.
_Avoid_: elevated manager, privileged input service
