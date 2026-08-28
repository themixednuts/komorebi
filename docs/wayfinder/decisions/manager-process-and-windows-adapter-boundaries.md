# Manager process and Windows adapter boundaries

## Decision

Use one authoritative manager process surrounded by replaceable, least-authority role processes. The manager alone commits manager state. A process boundary exists where code has different privilege, failure, latency, UI-thread, native-thread, or unsafe-runtime properties.

The stable startup reference launches the recovery authority. It selects the active installation, resolves an interrupted promotion, and starts the manager. The manager then launches and authenticates every role process for its own epoch. No child can claim a role from command-line text alone.

All production waits are event-driven. Window and desktop wakes come from WinEvent callbacks and window messages. Named pipes and filesystem watches use overlapped I/O. Process death uses waitable process handles registered with the Windows thread pool. Session changes use WTS messages. A desktop wake queues one observation pass. Wakes coalesce while that pass is pending, and a wake received during the pass queues one later pass. No runtime status poll, fixed settling sleep, retry tick, equality loop, or periodic path check is part of the architecture.

## Evidence and constraints

- [Choose manager state ownership and native effect boundaries](https://github.com/themixednuts/komorebi/issues/20) selected one revisioned owner, pure transitions, immutable snapshots, and typed effect plans.
- [Measure first-party input service operating limits](https://github.com/themixednuts/komorebi/issues/21) and [Prototype the input broker under privilege and device stress](https://github.com/themixednuts/komorebi/issues/25) proved a medium-integrity hook process with a narrow elevated broker. They also proved that pointer replacement and lossless input transitions must be separate data paths.
- [Decide the plugin model and trust boundary](https://github.com/themixednuts/komorebi/issues/17) selected one LPAC process per extension principal. [Prototype restricted extension-host containment and brokered I/O](https://github.com/themixednuts/komorebi/issues/39) proved authenticated named pipes, Job containment, brokered HTTPS and storage, native process waits, and lossless WTF-16 path handling on the target machine.
- [Define the manager-owned shell profile and Explorer recovery boundary](https://github.com/themixednuts/komorebi/issues/29) keeps Explorer available and requires shell features to fail independently. [Choose the UI toolkit adoption and migration route](https://github.com/themixednuts/komorebi/issues/9) makes GPUI the target renderer without giving it manager authority.
- [Define the versioned local command-catalog protocol](https://github.com/themixednuts/komorebi/issues/23) selected authenticated, framed, overlapped named pipes with one reader and one writer owner per connection.
- The current checkout confirms the migration pressure. `WindowManager` is shared through `Arc<Mutex<_>>`; `window_manager.rs` is 5,942 lines; `process_command.rs` is 2,487 lines; Win32 effects, transport handling, and process globals cross the same paths. This is precedent to remove, not preserve.
- A strict source scan found 24 `to_string_lossy` sites, five `to_str` sites, five `String::from_utf16` sites, 272 `unwrap` sites, 45 `expect` sites, and 48 discarded-result bindings across the current production package directories. These are audit candidates, not a claim that every site is wrong. Each migrated boundary must classify and remove operational loss, panic, or ignored failure rather than copy it into a new crate.

## Alternatives

### One process

The manager could host hooks, GPUI, LuaJIT, search, AppBars, and all Win32 adapters in one process. Calls would be cheap, but a renderer abort, Lua C API fault, blocked search, hook overload, or optional Windows integration would terminate or stall authoritative window management. It would also require the manager token to contain the union of every capability. Rejected.

### Process per module

Every feature could own state in a separate process and coordinate through messages. Failure isolation would be strong, but window topology, action admission, effect ordering, and recovery would become a distributed transaction. Rejected because manager invariants would lose one owner.

### Selected role isolation

Keep one state owner. Split processes only for a real privilege, failure, latency, UI, native-thread, or unsafe-runtime boundary. Role processes own local sessions and native resources, while the manager owns policy and truth. Selected because it isolates the proven hazards without distributing manager state.

Evidence can move a feature between existing role boundaries. It cannot create a second manager owner. A measured cross-process latency failure may move a pure worker into its consumer process. A privilege or crash-containment failure may move an adapter out. Either change keeps the same typed port.

## Runtime roles

| Role | Integrity and lifetime | Owns | Must not own |
| --- | --- | --- | --- |
| Recovery authority | Medium integrity, stable startup parent, independent of manager epoch | Active-installation selection, promotion journal, one manager process handle, bounded restart policy, Safe Stop | Manager config, window topology, Lua, GPUI, hooks, ordinary actions |
| Manager owner | Medium integrity, one per logon session | Authoritative state, revisions, catalog, action admission, policy, effect plans, role launch proofs, protocol authority | GPUI event loops, Lua callbacks, file indexing, HTTP, blocking pipe writes |
| AppBar host | Medium integrity, one process for the active AppBar role | AppBar HWNDs, `SHAppBarMessage`, monitor and DPI-local presentation, exact release record | Manager topology, application-window effects, palette state |
| Interactive shell host | Medium integrity, single instance | GPUI palette, overview, quick controls, DWM thumbnail presentation, focus capture projection, accessibility providers | Action admission, foreign-window truth, AppBar reservation, Lua |
| Notification host | Medium integrity, only while a valid exclusivity lease exists | Consented history projection and any proved single-presenter route | Notification policy authority, security notification suppression, manager state |
| OSD host | Medium integrity, one active proved route set | GPUI OSD presentation and route-local native resources | Unproved interception, system-wide manager authority |
| Input service | Medium integrity, one active input authority | Low-level hooks, Raw Input, device identity, input modes, lossless transitions, replaceable pointer sample | Window topology, native window effects, UI, elevated operations |
| Profile script host | Medium integrity, fresh process per prepared or active generation | One owner-profile LuaJIT VM and typed profile or automation session | Direct files, network, Win32, manager references, native modules |
| Extension host | Unique LPAC identity and Job, one process per extension principal | One text-only extension LuaJIT VM, package-local static input, bounded protocol session | Ambient filesystem, sockets, child processes, renderer callbacks, native handles |
| Capability broker | Medium integrity, manager-launched, replaceable | Extension private-storage transactions and policy-checked HTTPS execution on worker lanes | Grant policy, manager state, Lua, UI, elevated effects |
| Elevated effect broker | High integrity, on demand, short-lived | Exact typed HWND effects that medium integrity cannot perform because of UIPI | Hooks, config, UI, network, extension I/O, arbitrary process or command execution |
| Experimental Windows adapter | Medium integrity unless a narrower token works, replaceable | One documented but build-sensitive integration and its native resources | Authoritative state, fallback suppression after proof loss, unrelated Windows APIs |
| Compatibility client | Owner-session medium integrity | Parse legacy CLI or `SocketMessage` input and invoke the new control protocol | Direct manager mutation, direct Win32 effects, authority inferred from legacy input |

The shell executable may support several closed `ShellRole` variants, but one process instance receives one role. AppBar, notification presentation, and OSD integrations do not share a crash boundary with the interactive shell. The palette, overview, and quick controls share the interactive host because they use the same foreground, focus, GPUI, and accessibility session and are mutually coordinated.

`fff-search` stays behind the palette file-source port. Its benchmark decides whether the concrete engine runs inside the interactive host or in a contained search worker. Search never moves into the manager.

## Primitive contracts

These are code-shaped design contracts, not a new generic framework.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ManagerEpoch(uuid::Uuid);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProcessInstanceId(uuid::Uuid);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RoleGeneration(u64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManagedRole {
    Shell(ShellRole),
    InputAuthority,
    ProfileScript,
    Extension(ExtensionPrincipal),
    CapabilityBroker,
    ElevatedEffectBroker,
    ExperimentalAdapter(AdapterKind),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShellRole {
    AppBar,
    Interactive,
    Notification,
    Osd,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleLease {
    pub manager: ManagerEpoch,
    pub instance: ProcessInstanceId,
    pub generation: RoleGeneration,
    pub role: ManagedRole,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoleState {
    Starting(RoleLease),
    Ready(RoleLease),
    Draining(RoleLease),
    Stopped { generation: RoleGeneration, reason: StopReason },
    Disabled(DisableReason),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManagerInput {
    Control(AdmittedInvocation),
    Native(PlatformObservation),
    Input(InputTransition),
    Role(RoleEvent),
    Effect(EffectOutcome),
    Lifecycle(LifecycleEvent),
}

pub struct ProposedTransition {
    pub next: AuthoritativeState,
    pub events: CommittedEvents,
    pub effects: EffectPlan,
    pub result: TransitionResult,
}
```

`RoleLease` is manager-issued after the server verifies the process, token, session, expected PID, launch capability, and role-specific evidence. A disconnected or restarted process cannot reuse it. The manager accepts role output only when epoch, process instance, generation, and message sequence all match.

Do not begin with `dyn Process`, `dyn Service`, `dyn WindowsAdapter`, or a dependency bag. Each behavior gets a concrete operation and a consumer-owned port. Extract a trait only after several consumers need the same variable behavior.

## Thread and wait ownership

### Recovery authority

- The main thread owns startup selection, the promotion journal, and shutdown reporting.
- A Windows thread-pool wait observes the manager process handle. Exit delivery is callback driven.
- Restart is a state transition with an explicit backoff deadline. The deadline uses one waitable timer. It is not a polling loop.

### Manager owner

- One thread owns `AuthoritativeState` by value and processes ordered `ManagerInput` values.
- WinEvent callbacks copy bounded native facts into a nonblocking ingress queue and return. They never lock manager state or run policy.
- Native effect executors own any required COM apartment or HWND-affine thread. They receive immutable effect values and return typed outcomes.
- Named-pipe accept, read, and write owners use overlapped I/O. They cannot wait on the owner thread.
- Blocking compatibility filesystem work moves behind an adapter worker until deleted.

### Shell roles

- Each shell role has one GPUI or Win32 UI thread that owns its HWNDs and accessibility provider.
- DWM thumbnail and AppBar calls stay on the thread that owns the related shell window unless the API contract permits otherwise.
- Search, icon decode, capture preparation, and protocol I/O run off the UI thread and return generation-fenced values.

### Input service

- The low-level hook lives on a dedicated highest-priority message-loop thread.
- Raw Input lives on an owned message window thread.
- Transition events use a bounded lossless queue. Pointer motion uses a latest-value slot and one wake edge. Neither callback waits on IPC.
- Session and desktop boundary messages increment the input generation and synchronously clear local modes before later events can publish.

### Script and broker roles

- One Lua thread owns each `mlua::Lua`; Lua values never cross that thread or process boundary.
- Script event queues are bounded and generation fenced.
- HTTP and storage use owned asynchronous task sets in the capability broker. Cancellation, deadlines, and task failures are observed by the broker session.
- The manager owner receives only typed broker completion. It never executes network, storage, or Lua work.

## Windows text and path boundary

Windows filesystem names are sequences of potentially ill-formed UTF-16 code units. A Rust `String` cannot represent every valid name. The authoritative representation is therefore `PathBuf` or `OsString` inside a process and bounded WTF-16 code units on the Windows-only wire.

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsPath(std::path::PathBuf);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WireWindowsPath {
    units: Box<[u16]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowsPrefixKind {
    Disk,
    Unc,
    VerbatimDisk,
    VerbatimUnc,
}

#[derive(Debug, thiserror::Error)]
pub enum WindowsPathError {
    #[error("path is empty")]
    Empty,
    #[error("path contains a nul code unit")]
    InteriorNul,
    #[error("drive-relative paths are not accepted at this boundary")]
    DriveRelative,
    #[error("device namespace paths are not accepted at this boundary")]
    DeviceNamespace,
    #[error("path exceeds the operation limit")]
    TooLong,
}
```

Rules:

- Convert with `OsStrExt::encode_wide` and `OsStringExt::from_wide`. Never use `to_str`, `to_string_lossy`, `String::from_utf16`, or UTF-8 JSON for an operational path.
- Preserve UNC, verbatim UNC, verbatim disk, separator spelling, trailing dots and spaces, and unpaired surrogates until a specific operation defines a transformation.
- Use `std::path::Prefix` for native Windows prefix classification. The `verbatim` crate is suitable only for constructing an extended-length spelling after the operation has accepted an absolute path. It does not establish identity or security.
- `typed-path` is useful when parsing Windows syntax on a non-Windows host. Production runs only on Windows and needs native `OsString`, so it is not the authoritative runtime type.
- `wtf_string` may replace repeated `OsString` to wide-buffer conversions inside a measured hot Win32 adapter. It is not required for correctness and does not cross into domain types.
- Do not simplify a verbatim path with `dunce` before an authoritative operation. Display-only simplification cannot feed back into filesystem access.
- A path spelling never proves identity or containment. Open once with the required share, reparse, and creation flags, then authorize the resulting handle using volume and file identity. Subsequent work uses that handle or a handle-relative operation.
- Create private files and directories with their final access control at creation. New files use create-new semantics. Never check a path and then reopen it for a privileged mutation.
- Protocol codecs validate even byte length, code-unit count, interior NUL, accepted prefix class, and operation-specific bounds. Logs may use a lossy escaped display value, but that value cannot re-enter an operation.

This applies the defensive lessons from [Bugs Rust Won't Catch](https://corrode.dev/blog/bugs-rust-wont-catch/) to Windows rather than copying its Unix-specific implementation advice.

## Entrypoint-to-effect stacks

### Startup and role launch

```text
Windows Startup shortcut
  -> recovery::run(StartupReference)
    -> installation::resolve(PromotionJournal) -> ActiveInstallation | RecoveryError
      -> process::launch_manager(&ActiveInstallation) -> OwnedProcess
        -> process_wait::register(OwnedProcess) -> ManagerExitSubscription
      <- manager Ready(ManagerEpoch) | startup deadline | process exit
    <- healthy manager, one bounded restart, rollback, or Safe Stop report

ManagerOwner::start_role(RoleSpec)
  -> role_policy::admit(&AuthoritativeState, RoleSpec) -> AdmittedRole
    -> role_launch::prepare(AdmittedRole) -> LaunchCapability + ExpectedPeer
      -> windows_process::create_suspended(...) -> OwnedProcess
        -> role_containment::apply(...) -> ContainedProcess
          -> resume and await overlapped pipe connection
            -> peer::verify(kernel evidence, ExpectedPeer) -> RoleLease
      <- Ready(RoleLease) | typed launch failure
```

Recovery owns the manager restart decision. The manager owns every child role decision. A role startup timeout cancels the launch and closes its owned Job. It does not delay manager transitions.

### Native observation and manager effect

```text
WinEvent callback: RawWinEvent
  -> windows_observation::capture_bounded(RawWinEvent) -> ObservationWake
    -> observation_wake::enqueue_unless_pending(ObservationWake)
      -> manager ingress queue
        -> manager_owner::observe_once(ObservationWake) -> PlatformObservation | Unavailable
        -> transition::propose(&AuthoritativeState, ManagerInput::Native(...))
          -> manager_owner::commit(ProposedTransition) -> StateRevision + EffectPlan
            -> window_effects::execute(EffectPlan) -> EffectOutcome
              -> manager ingress queue
                -> manager_owner::settle(EffectOutcome)
```

The callback cannot claim what changed. One observation pass reads current Windows state. If another native wake arrives during that pass, the wake edge queues one later pass. Unavailable evidence preserves the prior observation. The manager never repeats reads to seek equal samples. Effect-specific HRESULT and Win32 errors become `EffectOutcome` at the adapter.

### Input to action

```text
hook or Raw Input callback
  -> input_adapter::decode(RawInput) -> InputTransition | PointerSample | Ignored
    -> input_session::apply(generation, event) -> ActionInvocation | ModeUpdate
      -> authenticated input pipe, nonblocking enqueue
        -> protocol::decode_and_authorize(InputEnvelope) -> AdmittedInvocation
          -> manager_owner::commit(action transition)
            -> effect dispatcher
          <- ActionSettlement
        <- correlated input acknowledgement
```

Queue saturation never drops a key or button transition. It disables capture into pass-through and reports overload. Pointer samples may replace older pointer samples before dispatch.

### Shell intent

```text
Windows UI input or UI Automation action
  -> gpui_projection::translate(RawShellInput) -> ShellIntent
    -> shell_session::apply(ShellIntent) -> SnapshotUpdate | ActionInvocation
      -> authenticated first-party control pipe
        -> manager action admission and transition
          -> native effect
        <- ActionSettlement
      -> shell_session::settle(ActionSettlement)
        -> GPUI pixels and UI Automation state
```

The shell UI thread never waits on the pipe or native effect. A stale result handle, shell generation, manager epoch, or state revision rejects before effect.

### Profile and extension scripts

```text
owner profile source handles
  -> profile_source::read_once(OpenedSourceSet) -> SourceClosure
    -> profile script host authenticated channel
      -> mlua_adapter::compile(SourceClosure) -> CandidateProfile | ScriptDiagnostic
        -> profile::validate(CandidateProfile) -> PreparedProfile
          -> manager_owner::commit_profile(PreparedProfile)

manager committed event
  -> authorized script projection
    -> profile or extension host queue
      -> mlua_adapter::invoke(TypedEvent) -> TypedActionRequest | ScriptFault
        -> authenticated role pipe
          -> canonical manager action admission
```

The profile reader opens source inputs before crossing into Lua and sends owned bytes plus native path labels. Extensions receive package content from validated handles. Neither Lua role can reopen an arbitrary path or manufacture its principal.

### Brokered extension I/O

```text
extension Lua request
  -> host codec -> StorageRequest | HttpRequest
    -> authenticated extension channel
      -> manager grant admission -> BrokerTicket
        -> capability broker pipe
          -> storage or HTTP adapter -> BrokerOutcome
        <- generation-fenced BrokerOutcome
      <- manager redaction and quota settlement
    <- Lua result table or typed error
```

The manager owns grant admission. The capability broker owns I/O, deadline, cancellation, redirect, address, and storage transaction mechanics. A broker crash returns `BrokerUnavailable`; it does not crash the manager or grant direct I/O to the host.

### Elevated effect

```text
manager committed effect requiring high integrity
  -> elevation_policy::admit(EffectPlanItem) -> ElevatedEffectRequest
    -> elevated broker launch or existing healthy short lease
      -> peer verification and consent evidence
        -> elevated_adapter::execute(ElevatedEffectRequest) -> ElevatedEffectOutcome
      <- typed outcome, deadline, disconnect, or uncertain completion
    -> manager reconciliation
```

Only idempotent convergent setters may retry after a proved pre-effect failure. A disconnect after possible mutation is `Unknown` and triggers observation. Toggles and close-like effects are never replayed blindly.

### Crash and Safe Stop

```text
process handle signals manager exit
  -> recovery::classify_exit(ExitStatus, PromotionState) -> RecoveryAction
    -> close manager child Jobs and wait handles
      -> one bounded restart or known-good rollback
        -> health gate
      <- Healthy | Failed
    -> safe_stop::restore(ExactRecoveryRecords)
      -> release AppBar and manager-owned effects
      -> leave or start Explorer
    <- aggregate RecoveryReport
```

Safe Stop attempts every independent restoration and reports the worst outcome. It never discards an earlier failure because a later cleanup passed.

## Failure and restart policy

| Failure | Result | Restart owner |
| --- | --- | --- |
| Manager exits | Role leases expire; input enters pass-through; shell roles release owned effects or exit; no secondary authority takes over | Recovery authority, once within the health policy, then rollback or Safe Stop |
| AppBar host exits | Windows releases the HWND; recovery broadcasts work-area change and verifies release from exact records; other shell roles remain | Manager while epoch is live, recovery during manager failure |
| Interactive shell exits | Window management continues; palette, overview, and quick controls are unavailable until a new generation is ready | Manager with bounded backoff |
| Input service exits or overloads | OS input continues normally; manager bindings become unavailable; held modes are discarded | Manager with a new generation |
| Profile script host faults | Active compiled profile remains; automation generation stops; no partial candidate commits | Manager after source or owner-triggered reload |
| Extension host faults | Its Job closes, contributions become unavailable, in-flight requests cancel, stale output rejects | Manager under the extension restart budget, then disable |
| Capability broker faults | Storage and HTTP calls fail typed; manager and extensions retain no direct fallback authority | Manager, independent of extension host restart |
| Elevated broker faults | Effect is rejected or marked unknown and reconciled; manager stays medium integrity | Manager on the next admitted request, never a blind effect retry |
| Experimental adapter faults or proof expires | Its feature returns to the documented Windows fallback before restart | Manager only after the feature gate is valid again |
| Recovery authority faults | Existing manager keeps running; the same stable executable can be invoked manually for Safe Stop | Windows startup on next logon or owner invocation |

## Crate and module placement

```text
komorebi-control
  manager input, actions, revisions, transitions, effect plans

komorebi-protocol
  role leases, authenticated envelopes, codecs, bounded Windows path wire type

komorebi-manager
  owner loop, role policy, catalog, protocol sessions, effect dispatch

komorebi-windows
  small safe wrappers grouped by consuming capability
  observation, window_effects, process_wait, role_launch, path_handles

komorebi-shell-core
  palette, overview, quick controls, AppBar snapshots and intents

komorebi-shell
  ShellRole composition roots, GPUI projection, HWND and UI Automation adapters

komorebi-input
  hook and Raw Input threads, modes, bounded delivery

komorebi-script-host
  profile and extension role roots, the only mlua dependency

komorebi-capability-broker
  private storage and HTTPS concrete adapters

komorebi-elevated-broker
  closed elevated effect union and Win32 implementation

komorebi-recovery
  active installation, promotion journal, manager wait, rollback, Safe Stop
```

This is a target dependency direction, not an instruction to create empty crates. Begin with focused modules. Extract a crate when its process binary, dependency exclusion, security review, or protocol stability makes the boundary real.

The current `process_command.rs` match cannot become the new manager loop with more branches. Migrate one behavior at a time into typed application operations. Delete its compatibility arm in the same change that migrates the last caller. Likewise, process globals move into their owning state or role composition root instead of receiving another synchronization wrapper.

## Proof obligations

1. Start each role and prove that claimed command-line role text without the expected PID, token, launch capability, epoch, and generation receives no authority.
2. Kill every role independently. Verify the failure result and restart owner in the table, one active generation, no duplicate hotkey, HWND, AppBar, presenter, or effect.
3. Kill the manager at every promotion phase. Verify role lease expiry, input pass-through, exact shell cleanup, one recovery decision, and Explorer usability.
4. Saturate every queue. Verify no lost key or button transition, explicit pointer replacement, nonblocking manager publication, and typed lag or resynchronization.
5. Deliver duplicate desktop and window wakes before and during an observation pass. Verify pre-pass wakes coalesce, an in-pass wake schedules exactly one later pass, and no timer, equality sampling, or status poll occurs.
6. Exercise pipe disconnect before effect, after effect, and before outcome. Verify retry only when pre-effect failure is proved and reconciliation for uncertain completion.
7. Pass disk, UNC, verbatim disk, verbatim UNC, trailing-dot, trailing-space, long, and unpaired-surrogate paths through CLI, protocol, storage, recovery, and Win32 round trips. The exact code units must survive.
8. Attempt interior NUL, drive-relative, device namespace, reparse swap, junction swap, alternate data stream, and check-then-replace attacks at every privileged path boundary. Verify rejection or handle-anchored identity.
9. Audit operational code for `to_string_lossy`, operational `display()` round trips, `to_str`, `String::from_utf16`, `unwrap`, discarded `Result`, path prechecks, `File::create`, polling sleeps, and raw Win32 types above adapters.
10. Keep unsafe code in the Win32 and mlua adapters, document each local proof, and run Clippy with warnings denied plus Miri or targeted adversarial tests where the boundary permits it.
11. Enforce dependency rules. The manager cannot depend on GPUI, `mlua`, `fff-search`, HTTP clients, or elevated-broker implementation types. Shell and script hosts cannot depend on manager internals.

The implementation is not complete when these tests pass only against mocks. Each process, token, pipe, Job, HWND, wait handle, recovery record, and path family needs at least one Windows 11 vertical test on the target installation.
