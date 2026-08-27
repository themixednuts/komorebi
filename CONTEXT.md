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

**Control surface**:
The first-party interface for finding, understanding, and invoking entries from the command catalog.
_Avoid_: shortcuts window, general-purpose launcher

**Command palette**:
The first-party searchable shell window that combines command-catalog actions with separately typed application, file, file-content, and explicit web search sources. Search results do not become manager commands.
_Avoid_: shortcuts window, PowerToys adapter

**Search source**:
A typed source of command-palette results with its own query, privacy, ranking, and activation rules.
_Avoid_: command type, untyped result list

**Lua extension**:
An owner-installed script that observes manager events and requests typed manager actions through an explicit capability grant. It does not own authoritative state or receive raw native handles.
_Avoid_: native plugin, arbitrary manager code

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
