# Accessible manager-owned shell host

## Decision

Build one shared Rust shell implementation and run it as four independently supervised GUI-subsystem role processes: AppBar, interactive, notification, and OSD. The palette, overview, and quick controls share the interactive role because they have one mutually exclusive foreground/focus session. AppBar, notification, and OSD stay in separate processes because they own different Windows resources, activation policies, proof leases, and failure recovery.

The roles share code and immutable values, not mutable memory. The manager remains the only authoritative state owner. A role owns its GPUI event loop, HWNDs, UI Automation provider, role-local session, and documented native registrations. GPUI projects pixels and accessibility state; it does not own manager truth, action admission, native effects, restart policy, or recovery.

All production observation is event-driven. Window messages, WinRT setting events, named-pipe completions, UI Automation requests, and native process waits invalidate a role-local snapshot. The UI thread performs one observation or projection pass for each coalesced invalidation. There is no status poll, settling burst, fixed sleep, equality loop, or periodic topology/settings check.

The current egui bar is a migration reference only. GPUI is the sole target renderer. No renderer trait, cross-toolkit widget tree, runtime toolkit switch, or permanent fallback implementation is introduced.

## Grounded constraints

- [Choose manager process and Windows adapter boundaries](https://github.com/themixednuts/komorebi/issues/19) already selected one manager owner and the same four shell roles. This decision deepens that boundary rather than creating another process model.
- [Choose the UI toolkit adoption and migration route](https://github.com/themixednuts/komorebi/issues/9) selected GPUI, with GPUI `797e5dc95c3859f7926681c91398c4d9e993865d` and GPUI Components plus `gpui-base` `6d07863fe7077f85abfa0ec2fcb05f3e17c573b2` pinned and reviewed together.
- [Benchmark equivalent egui and GPUI Components control surfaces](https://github.com/themixednuts/komorebi/issues/8) measured lower GPUI idle CPU and working set, but found an incorrectly reported text-input focus and no selected-result state in the benchmark surface. These are implementation gaps, not permission to ship an inaccessible candidate.
- The pinned GPUI source handles `WM_GETOBJECT` through AccessKit and exposes stable `accessibility_id`, `aria_selected`, `aria_active_descendant`, roles, labels, values, and accessible actions. The host must use and verify those primitives rather than assume toolkit integration makes a surface accessible.
- Microsoft requires custom controls to expose a server-side UI Automation provider, properties, appropriate control patterns, navigation/focus, and events. The provider root is returned for `UiaRootObjectId` in `WM_GETOBJECT`; the default HWND proxy is not sufficient for the custom contents.
- The target remains an Explorer-coexisting personal Windows 11 shell. DWM, Explorer recovery, Start, the tray host, secure desktop, and unsupported Windows shell surfaces remain outside this host.
- The real target installation currently has one monitor. Mixed-DPI and monitor removal are release gates that require a second physical or virtual display arrangement; they cannot be waived by a single-monitor unit test.

## Alternatives

### One process for every shell surface

This makes in-process state sharing easy, but a palette renderer fault could remove the AppBar reservation and an OSD adapter fault could terminate notification history. It also forces unrelated activation, COM, HWND, and recovery policies into one conditional composition root. Rejected.

### One process per feature or window

This maximizes isolation but makes palette, overview, quick controls, and their flyouts coordinate one foreground interaction across several processes. It duplicates GPUI roots, accessibility trees, focus restoration, environment observation, and protocol sessions. Rejected.

### One shared implementation, split by Windows role

This preserves one interactive session where it is genuinely shared and separates the native resources whose failure behavior differs. It is selected. Evidence may move a future feature between the four existing roles, but cannot create a second manager owner or silently combine failure domains.

## Primitive contracts

These are focused contracts, not a generic UI framework.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShellRole {
    AppBar,
    Interactive,
    Notification,
    Osd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ShellGeneration(u64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellLease {
    pub manager: ManagerEpoch,
    pub process: ProcessInstanceId,
    pub generation: ShellGeneration,
    pub role: ShellRole,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppBarLease(ShellLease);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InteractiveLease(ShellLease);

#[derive(Debug, Clone, PartialEq)]
pub struct ShellEnvironment {
    pub revision: EnvironmentRevision,
    pub displays: NonEmptyDisplays,
    pub contrast: ContrastPresentation,
    pub text_scale: TextScale,
    pub motion: MotionPreference,
    pub theme: ShellThemeTokens,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotionPreference {
    Enabled,
    Reduced,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SemanticNodeId(AsciiSemanticId);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusReturnTarget {
    pub window: WindowId,
    pub observed_generation: WindowGeneration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FocusReturnOutcome {
    Returned,
    TargetGone,
    DeniedByWindows,
    SupersededByUser,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppBarReady {
    pub lease: AppBarLease,
    pub first_frame: PresentedFrameId,
    pub accessibility_root: SemanticNodeId,
    pub reservations: AppBarReservations,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InteractiveReady {
    pub lease: InteractiveLease,
    pub first_frame: PresentedFrameId,
    pub accessibility_root: SemanticNodeId,
    pub focus: InteractiveFocus,
}
```

The protocol dispatcher selects a role-specific codec only after authentication. An AppBar connection accepts `AppBarSnapshot` and `AppBarIntent`; an interactive connection accepts `InteractiveSnapshot` and `InteractiveIntent`; notification and OSD do the same with their concrete values. There is no generic role payload carried through rendering code and no role/snapshot combination for downstream code to recheck.

`SemanticNodeId` is stable across frames and derived from a domain identity, never a row index, localized label, HWND, path spelling, or toolkit allocation. It maps to GPUI's internal element ID and to `accessibility_id` for Windows UIA `AutomationId`. `UiText` is valid display text. It cannot carry a native path or become an activation operand.

Checked role-lease newtypes are constructed only by role authentication. Each role has a concrete readiness value such as `AppBarReady` or `InteractiveReady`, so its required resources cannot be confused with another role's. It is emitted only after authentication, HWND creation, exact native-resource acquisition, a complete first frame, and accessibility-root activation. “Process exists” and “window is visible” are not readiness.

## Ownership and leak rules

| Owner | Owns | Must not know or own |
| --- | --- | --- |
| Manager | Authoritative window state, revisions, policy, action admission, role leases, role restart decisions | GPUI entities, AccessKit nodes, HWNDs, UIA interfaces, role-local focus |
| Shell core | Role-specific snapshots, intents, semantic identities, selection/focus rules, theme tokens | GPUI, egui, `windows` types, raw paths as UI strings |
| Role host | One role-local session, process protocol, worker cancellation, environment revision, readiness | Manager internals, another role's native resources |
| GPUI projection | Elements, layout, pixels, semantic projection, accessible-action translation | Manager mutation, native registrations, restart policy |
| Windows host adapter | HWND creation/styles, activation, UIA bridge, DPI/topology/settings observation, AppBar/DWM calls | Catalog policy, result ranking, manager state |
| Supervisor | Process launch proof, native process wait, generation fencing, restart/backoff, Safe Stop handoff | Rendering or feature state |

The composition root constructs concrete adapters. Do not add `dyn ShellService`, `dyn Renderer`, a dependency bag, or a generic widget model. A consumer-owned port is extracted only for behavior that actually varies, such as file search or a native effect adapter.

The executable uses the Windows GUI subsystem, so login and restart never flash a console. A hidden role argument may select a closed composition root, but text on a command line grants no authority. The manager-issued launch capability, expected PID, token/session evidence, lease, and generation do.

## Accessible presentation contract

Visual state, keyboard state, and UI Automation state are built from the same concrete feature snapshot in the same GPUI element construction. There is no renderer-neutral accessibility tree or parallel UIA model to synchronize. A control cannot be visually selected while its GPUI semantic properties report otherwise. An accessible action produces the same typed intent as keyboard or pointer input; it never calls a separate manager command path.

Minimum semantic rules:

- Every meaningful control has a stable `SemanticNodeId`, correct control type/role, accessible name, enabled/focusable state, and only the patterns/actions it actually supports.
- The palette query exposes a text-input value and real keyboard focus. Results expose a selection container and selectable items. The selected item reports selected state; when focus remains on the query or composite container, the chosen result is the active descendant.
- Bar buttons and toggles expose invoke or toggle behavior. Decorative text and icons are absent from the control view unless they add meaning.
- Overview items expose stable window-derived identities and names without using a title as identity. Closing, focusing, or moving a window is an explicit accessible action with the same admission checks as any other caller.
- Notification and OSD projections are non-activating by default. Changes important to a screen-reader user use the appropriate UIA event or live-region/notification behavior; they do not steal keyboard focus.
- Quick-control values expose range, toggle, selection, or invoke semantics as appropriate. A visual gesture with no keyboard and accessible equivalent blocks release.
- UIA provider calls do not wait on manager IPC, search, icon decoding, capture, disk, or network. GPUI/AccessKit answers from the most recent complete projection and role snapshot without initiating application work.
- Provider teardown disconnects cleanly with the HWND. No COM/UIA object may retain GPUI entities or a dead role generation.

The Windows adapter continues to use AccessKit through GPUI while it satisfies these rules. If a required Windows pattern, property, event, or stable identity cannot be expressed at the reviewed GPUI revision, extend the narrow GPUI/AccessKit boundary or upstream it. Do not ship a parallel hand-written UIA tree that can drift from the rendered tree.

## Keyboard and focus contract

Each role has one activation policy:

| Role | Default focus behavior |
| --- | --- |
| AppBar | Non-activating during ordinary presentation. An explicit keyboard-entry action opens a focusable bar session; pointer and UIA actions invoke exact controls without an unsolicited focus jump. |
| Interactive | Captures a `FocusReturnTarget` before activation, opens one palette/overview/quick-controls session, and puts real keyboard focus on its initial semantic control before readiness. |
| Notification | Popup/history presentation does not steal focus. Explicit history invocation opens a focusable session and captures a return target. |
| OSD | Never activates or enters the tab order. Any future interactive OSD must become an explicit proved mode rather than a flag in the passive path. |

Tab and Shift+Tab traverse every interactive control in a stable logical order. Arrow keys move within composite controls, Enter invokes the selected action, Escape closes the top interaction, and standard text editing/IME behavior remains with the text input. Key handling is expressed as typed intents and does not depend on screen coordinates.

On close, the Windows adapter revalidates the captured `WindowId` and generation. It attempts one documented foreground return only if the user has not focused something else. Windows may deny activation; that is the terminal `DeniedByWindows` outcome, not a reason for `AttachThreadInput`, synthetic input, a retry loop, or a focus war.

## DPI, display, contrast, text, and motion

The role process establishes Per-Monitor V2 DPI awareness before creating any HWND. Physical monitor rectangles and logical GPUI units remain distinct types. `WM_DPICHANGED` supplies the new DPI and suggested top-level rectangle; `WM_DISPLAYCHANGE` and the relevant device/display notifications invalidate topology. One topology pass enumerates current monitors, preserves stable monitor matching where Windows provides enough evidence, and explicitly reports removed or ambiguous targets.

The environment adapter observes:

| Concern | Event source | Re-observation |
| --- | --- | --- |
| Per-window DPI | `WM_DPICHANGED` | New DPI and suggested rectangle; rebuild DPI-dependent resources once |
| Display topology | `WM_DISPLAYCHANGE` plus documented device/display notifications | Enumerate the current topology once |
| High contrast/theme | `WM_SETTINGCHANGE`, `WM_THEMECHANGED` | `SystemParametersInfoW(SPI_GETHIGHCONTRAST)` and system colors/theme values |
| Text scale | `UISettings::TextScaleFactorChanged` | `UISettings::TextScaleFactor` |
| Reduced motion | `UISettings::AnimationsEnabledChanged` | `UISettings::AnimationsEnabled`; Win32 fallback uses `SPI_GETCLIENTAREAANIMATION` on its setting message |
| Color/effects | `UISettings` color/effect change events where used | Rebuild only affected theme tokens |

Callbacks enqueue a coalesced `EnvironmentInvalidated` wake and return. They never mutate GPUI state from a foreign thread. The UI thread re-observes the setting, creates one new `ShellEnvironment`, and projects one revision. Duplicate events before the pass collapse; an event during the pass schedules one later pass. That is event coalescing, not polling.

High contrast resolves colors from current Windows system values; custom theme colors do not override them. Standard themes still meet at least 4.5:1 for ordinary text. Text scale supports the Windows 100% through 225% range with reflow, clipping only at the end, and full accessible names for clipped labels. Motion presentation becomes immediate state change when animations are disabled; removing animation cannot remove information or change committed behavior.

## Entrypoint-to-effect stacks

### Role startup

```text
recovery/manager composition root: PreparedRoleLaunch
  -> supervisor::launch_role: ExpectedProcess + one-use LaunchCapability
    -> GUI-subsystem komorebi-shell composition root: RawRoleLaunch
      -> role_launch::authenticate: ShellLease | RoleLaunchError
        [named-pipe hop; validates expected PID, user/session/token, manager epoch, role, generation]
        -> windows_host::create_role_windows_hidden
          [role UI thread; owns HWND styles and COM/WinRT apartment]
          -> concrete role session from initial role snapshot
            -> role-native adapter acquires exact AppBar/DWM/notification/OSD resources
            -> gpui_projection::present_and_show_complete_frame
              -> GPUI/AccessKit publishes pixels and semantic root
          <- concrete AppBarReady | InteractiveReady | NotificationReady | OsdReady | RoleStartError
        <- readiness protocol reply
      <- failed startup releases acquired resources before process exit
    <- native process handle registered with supervisor wait
```

No frame above trusts a self-declared role. Startup errors name the failed phase and any cleanup outcome. A partially acquired native resource cannot be hidden by returning a generic startup failure.

### Manager snapshot to accessible frame

```text
authenticated interactive-role pipe completion: InteractiveSnapshotEnvelope
  -> protocol::decode_interactive_snapshot: InteractiveSnapshot | ProtocolError
    [bounded frame; exact lease/generation/sequence; off UI thread]
    -> role_host::publish_snapshot: SnapshotInvalidation
      [latest complete snapshot slot + one wake edge; stale revisions rejected]
      -> concrete session::apply_snapshot
        [UI thread; pure state transition]
        -> gpui_projection::project_visual_and_semantic_elements
          -> Windows host presents one frame and AccessKit tree update
        <- PresentedFrameId | PresentationError
```

Superseded snapshots may be replaced before rendering, but committed intents and input transitions are not silently dropped. Bounded protocol or projection failure keeps the previous complete frame or closes the role with a typed degradation; it never exposes a half-applied tree.

### Keyboard, pointer, or assistive action

```text
GPUI key/pointer callback or AccessKit action: RawSurfaceInput
  -> concrete interactive projection adapter: InteractiveIntent | InputError
    -> concrete role session::apply_intent: LocalProjection | ManagerInvocation
      -> local-only intent: update snapshot and accessible presentation atomically
      -> manager invocation: authenticated named-pipe request
        [exact action/result/window identity; async; UI thread never blocks]
        -> manager application operation: admitted result | ActionError
      <- generation-fenced completion updates role snapshot or presents typed error
```

UIA invocation, Enter, and pointer click converge before the manager boundary. No caller can bypass action admission by choosing a different input mechanism.

### Environment change

```text
window message or WinRT setting event: NativeEnvironmentWake
  -> windows_host::classify_environment_wake: EnvironmentInvalidation
    [callback/message thread; coalesce and enqueue only]
    -> environment::observe_invalidated_fields
      [role UI thread; one supported API read per affected field]
      -> ShellEnvironment revision
        -> concrete role session::apply_environment
          -> layout/theme/semantic projection
            -> complete GPUI frame
```

An unavailable setting is an explicit observation failure. It retains the last complete environment or blocks readiness according to the affected gate; it is never replaced with a magic DPI, color, text scale, or animation value.

### Role death and recovery

```text
role process handle becomes signaled
  -> supervisor native process-wait callback: RoleExited
    -> manager/recovery lifecycle transition validates lease generation
      -> revoke role authority and publish feature unavailable
      -> role-specific cleanup/reconciliation
        -> AppBar: observe work area and let Explorer release dead-process registration
        -> interactive: discard session; never reclaim focus from the user's new foreground
        -> notification: revoke presenter lease before any replacement starts
        -> OSD: revoke every proved route; Windows fallback remains authoritative
      -> restart policy chooses stop, immediate restart, or one waitable backoff deadline
        -> launch a fresh process instance and generation
```

The waitable deadline is a single supervisor state transition, not a status poll. Repeated failure reaches typed disabled/Safe Stop behavior instead of an infinite crash loop.

## Windows strings and paths

Windows-native paths remain `Path`/`PathBuf` or `OsStr`/`OsString` inside a process and bounded WTF-16 code units on the Windows-only wire. This preserves UNC, verbatim UNC, verbatim disk, trailing-dot/space spellings, and unpaired surrogates. Rust's Windows implementation uses WTF-8 internally, but code must not depend on that private byte representation; Win32 boundaries use `OsStrExt::encode_wide` and `OsStringExt::from_wide` for a lossless round trip.

`std::path::Prefix` is the authoritative syntax classifier on Windows. The focused `verbatim` crate may construct an extended-length spelling only inside an adapter for an operation that requires it. It is not a normalizer, identity check, authorization decision, or reason to rewrite `/` inside an already-verbatim path. The `wtf_string` crate supplies explicit `Wtf8String` and `Wtf16String` types and is the preferred candidate if measurement justifies avoiding repeated wide-string conversion in a hot Win32 adapter; it is not a domain type, and adopting its young API requires its own review and benchmark.

The UI never activates a file from rendered text. A file result carries an opaque stable result identity and a lossless native path operand. Its separately derived `UiText` may escape or replace an unrenderable surrogate for display, but that label can never be parsed back into a path. UI Automation names follow the same rule.

The defensive checklist from [Bugs Rust Won't Catch](https://corrode.dev/blog/bugs-rust-wont-catch/) applies at every migrated boundary:

- reject invalid external input with typed errors rather than `unwrap`, `expect`, indexing, unchecked casts, or a process panic;
- do not discard a meaningful `Result`; an intentionally ignored failure has a local safety explanation;
- never use `to_string_lossy`, operational `display()`, `to_str`, or `String::from_utf16` to round-trip a native path;
- compare opened file/volume identity for authority or containment, not path strings, canonical spellings, or pre-checks;
- open once with operation-appropriate flags and authorize the handle, avoiding check-then-open and reparse races;
- create private durable files with final access control and create-new/replace semantics; do not make a sensitive file public and fix it afterward;
- prove error, cancellation, partial-write, full-disk, duplicate-delivery, and restart behavior, not only happy-path type safety.

## Failure rules

- A role cannot publish or invoke after its lease, manager epoch, process identity, generation, or message sequence becomes stale.
- A blocked search, icon decode, thumbnail preparation, protocol write, disk operation, or network handoff never runs on a role UI/UIA thread.
- Worker results carry role, session, and request generations. Late completion is discarded as an observed stale result, not applied to a newer session.
- Cancellation is owned by the role session. Closing an interaction cancels its workers and makes later results unrepresentable as current.
- Queue saturation is explicit. Replaceable pointer/environment snapshots use latest-value slots; lossless intent/control paths use bounded admission or typed overload and never silently drop a transition.
- An accessibility projection failure blocks candidate promotion. It cannot fall back to a visually correct but semantically empty window.
- AppBar cleanup is exact and role-local. Notification or OSD proof loss disables manager presentation before restart. Interactive failure leaves manager state and Explorer usable.
- Explorer restart re-registers only the AppBar role for the new Shell generation. It does not restart every shell role or trigger a polling recovery pass.
- Unknown native state fails open: no foreign window mutation, no duplicate notification/OSD presentation, and no invented monitor or focus target.

## Module placement

Begin as focused modules and extract crates only when the process/dependency boundary is real:

```text
komorebi-shell-core/
  environment.rs        typed environment values
  accessibility.rs      semantic identities and shared action vocabulary; no UI tree
  appbar.rs              AppBar snapshots and intents
  interactive.rs         palette/overview/quick-control session union
  notification.rs        notification snapshots and intents
  osd.rs                 OSD snapshots and intents

komorebi-shell/
  main.rs                GUI-subsystem composition root; closed role dispatch only
  role_launch.rs         manager launch proof and lease authentication
  appbar/host.rs         AppBar role session
  interactive/host.rs    one foreground/focus session
  notification/host.rs   history/presenter role session
  osd/host.rs            proved OSD route session
  gpui/                   concrete role projections and semantic adapters
  windows/               HWND, UIA/AccessKit, DPI, settings, focus, AppBar/DWM adapters

manager supervisor
  shell_roles.rs         leases, readiness, native waits, restart decisions
```

Do not put this into the existing `komorebi-bar/src/bar.rs`, which is already over 1,400 lines, or grow `process_command.rs`. The structural simplification is to make role composition roots closed and concrete, then delete the egui bar and shortcuts implementation after their callers move. No new shell feature adds a branch to the legacy renderer.

## GPUI cutover and egui deletion gates

The differential harness replays the same renderer-neutral snapshots and intents against the active egui reference and staged GPUI candidate. Only one candidate may own live bindings, HWND effects, AppBar reservations, or presentation authority at a time.

Promotion requires all of the following on the target Windows 11 installation:

1. **Behavior:** every accepted legacy snapshot/intent scenario has the same manager-visible result, or an explicitly approved improvement. Workspace selection, layout actions, widgets, pointer input, flyouts, and configuration-derived appearance are covered.
2. **UI Automation:** Inspect/Accessibility Insights sees the correct control/content tree, stable AutomationIds, names, roles, enabled/focusable state, patterns, selection, active descendant, values, bounds, and events. No duplicate IDs or frame-to-frame identity churn occur.
3. **Narrator:** a keyboard-only script opens, reads, navigates, selects, invokes, closes, and restores focus for every interactive surface. Notification/OSD announcements do not steal focus or repeat on unrelated frames.
4. **Keyboard and IME:** Tab order, arrows, Enter, Escape, editing, composition, surrogate pairs, dead keys, and layouts used by the owner work without raw-key shortcuts corrupting text input.
5. **DPI and topology:** move each surface between at least 100% and a different scale, remove/reconnect a monitor, change the primary monitor, rotate where supported, and restart Explorer. Bounds, hit testing, UIA bounds, text, icons, AppBar reservation, and focus remain correct.
6. **Accessibility settings:** switch high contrast/theme, text scale through 225%, and animations off while each surface is open. The event-driven update is immediate, reflow remains usable, system colors win, and motion can disappear without lost state.
7. **Crash isolation:** kill each role at startup phases and during interaction. Other roles and Explorer remain usable; one fresh generation starts; no duplicate reservation, hotkey, notification presenter, OSD, or stale focus return survives.
8. **Resources:** for the equivalent palette, retain the inherited review thresholds of at most 350 ms median cold first frame, 0.20% median idle CPU, and 80 MB median working set. Any regression requires new measured evidence and an explicit decision, not a silent threshold change.
9. **AppBar/native parity:** the GPUI bar passes the native AppBar lifecycle proof, uses one reservation per monitor/role generation, produces no console flash, and never paints into an unreserved work area during promotion.
10. **Lossless boundary cases:** disk, UNC, verbatim disk/UNC, long, trailing-dot/space, and unpaired-surrogate paths survive catalog-to-activation round trips. Interior NUL, drive-relative, device namespace, reparse/ADS ambiguity, stale result identity, and unsafe containment attempts are rejected or handle-anchored.
11. **Error paths:** forced pipe disconnect, UIA client disconnect, device loss, renderer failure, queue saturation, full disk during durable output, setting-observation failure, cancellation, and partial startup produce typed outcomes and complete cleanup.
12. **Source audit:** strict Clippy and tests pass; the migrated path contains no operational lossy path conversion, meaningful discarded result, polling sleep, raw Win32 type above adapters, toolkit type in shell core, or new legacy renderer dependency.

After a surface passes these gates, promotion atomically assigns GPUI as its only active renderer. The same source change deletes that surface's egui implementation, migration branches, and eframe dependencies. The AppBar/bar cutover is last; afterward `cargo tree -i eframe` must report no workspace consumer.

## Supported limits

This host does not replace DWM, Explorer, Start, arbitrary tray items, Notification Center, Quick Settings, secure desktop, or foreign application rendering. It cannot guarantee `SetForegroundWindow` success against Windows foreground policy. It does not claim manager notification or OSD presentation until those routes hold separate live proof leases. UI Automation quality is a property of our semantic model and tests, not a blanket guarantee supplied by GPUI or AccessKit.

## Primary references

- [Microsoft: UI Automation providers overview](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-providersoverview)
- [Microsoft: implement a server-side UI Automation provider](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-serversideprovider)
- [Microsoft: handling `WM_GETOBJECT`](https://learn.microsoft.com/en-us/windows/win32/winauto/handling-the-wm-getobject-message)
- [Microsoft: UI Automation specification](https://learn.microsoft.com/en-us/windows/win32/winauto/ui-automation-specification)
- [Microsoft: `WM_DPICHANGED`](https://learn.microsoft.com/en-us/windows/win32/hidpi/wm-dpichanged)
- [Microsoft: accessible text requirements](https://learn.microsoft.com/en-us/windows/apps/design/accessibility/accessible-text-requirements)
- [Microsoft: text scaling](https://learn.microsoft.com/en-us/windows/apps/develop/input/text-scaling)
- [Microsoft: `UISettings`](https://learn.microsoft.com/en-us/uwp/api/windows.ui.viewmanagement.uisettings)
- [GPUI accessibility source at the reviewed revision](https://github.com/zed-industries/zed/tree/797e5dc95c3859f7926681c91398c4d9e993865d/crates/gpui)
- [`verbatim` Windows extended-path adapter](https://docs.rs/verbatim/latest/verbatim/)
- [`wtf_string` explicit WTF-8/WTF-16 storage](https://docs.rs/wtf-string/latest/wtf_string/)
