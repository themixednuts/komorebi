# Lua extension-host call stack

## Selected boundary

Each extension receives one Rust-owned `mlua` LuaJIT VM inside its eventual
LPAC worker process. The VM is not shared across extensions, renderers, or the
manager. Production starts in the explicit `JitDisabled` profile: instruction
hooks cannot bound compiled LuaJIT traces, so JIT-on remains unavailable until
the separate LPAC containment gate proves preemption, memory, mitigations, and
broker ownership together.

The VM loads only table, string, math, and bit libraries. LuaJIT's `jit` module
is loaded privately long enough for the host to call `jit.off()` and
`jit.flush()`, then omitted from the extension environment. The environment is
an allowlist and contains no `io`, `os`, `package`, `require`, `debug`, `ffi`,
`jit`, `dofile`, `loadfile`, `load`, process, registry, COM, filesystem, or
network authority.

## Typed stack

```text
LPAC worker bootstrap
  -> PluginManifest { PluginId, PluginCapabilitySet }
  -> PluginVm::new(manifest, PluginLimits, broker ports)
    -> Lua::new_with(minimal libraries + private JIT control)
      -> host-owned jit.off + jit.flush
      -> absolute allocator ceiling from current VM bytes + MemoryBudget
      -> count hook from InstructionBudget
      -> allowlisted per-extension environment
        -> PluginContext UserData [capability-checked methods]

extension UTF-8 source bytes
  -> PluginProgram::new [reject invalid UTF-8 / binary Lua chunks]
  -> PluginVm::load(PluginProgram)
    -> ChunkMode::Text [defense in depth]
    -> parse/evaluate in the allowlisted environment
      -> returned module table
        -> required `on_load(context)` callback
          -> PluginContext::info/debug/warn/error/trace(message)
            -> PluginCapabilitySet::allows(Log)
              -> PluginLogSink [consumer-owned broker port]
                -> accepted structured record | typed denial/failure
    <- Loaded | typed syntax/API/budget/memory failure
```

`PluginCapabilitySet` is one closed bitset over `PluginCapability`; it does not
use independent booleans that permit contradictory states. `PluginId`, memory,
instruction limits validate at construction. Plugin programs accept UTF-8 text
only and the VM independently forces text mode, so precompiled chunks cannot
bypass source-level policy. Script bytes are never interpreted as a Windows
path. Rust panics are resumed at the Rust boundary rather than made catchable
by Lua.

The in-process VM contract remains independently testable with consumer-owned
fake ports. Production source is admitted only to the isolated worker below.

The native containment seam now proves the worker process itself:

```text
PluginId
  -> SandboxIdentity [stable profile identity per plugin]
  -> LpacWorkerLauncher::launch_probe(worker path)
    -> CreateAppContainerProfile | DeriveAppContainerSidFromAppContainerName
    -> STARTUPINFOEX attribute list
      -> SECURITY_CAPABILITIES [zero ambient capabilities]
      -> ALL_APPLICATION_PACKAGES opt-out [LPAC]
      -> child-process restricted
      -> immutable Win32k/dynamic-code/extension-point/strict-handle mitigations
    -> CreateProcessW [explicit absolute image, suspended, inherits zero handles]
    -> Job Object [one active process, kill on owner close]
    -> assign before resume
    -> trusted worker containment probe
      -> AppContainer token + LPAC access-check behavior
      -> low integrity + zero capability groups
      -> child/Win32k/dynamic-code mitigations
      -> Job membership
    <- VerifiedLpacWorker typestate | typed rejection
```

The standalone probe remains a live Windows integration proof. The production
host uses the same token, mitigation, and Job assertions before announcing
readiness on its allowlisted broker channel.

## Brokered worker session

```text
PluginHostService::start
  -> tokio::task::spawn_blocking(run_plugin_owner)
    -> LpacWorkerLauncher::launch_session
      -> CreatePipe [broker/worker one-way pairs]
      -> PROC_THREAD_ATTRIBUTE_HANDLE_LIST [worker ends only]
      -> CreateProcessW [suspended LPAC, empty environment]
      -> AssignProcessToJobObject
      -> ResumeThread
    -> WorkerSession::await_ready
    -> WorkerSession::initialize
      -> bounded, versioned wire request
      -> worker_runtime::run
        -> run_worker_containment_probe
        -> PluginVm::new + PluginVm::load
        -> bounded, versioned wire response
  -> publish PluginHostClient only after the initial program loads

PluginHostClient::reload
  -> acquire bounded admission permit
  -> mpsc::Sender<OwnerCommand>::send
  -> blocking owner completes WorkerSession::reload
  -> worker loads a replacement PluginVm
  -> worker swaps the VM only after successful load
  -> oneshot reply [caller cancellation drops result interest only]

PluginHostService::shutdown
  -> close admission
  -> owner sends wire shutdown
  -> worker acknowledges and exits
  -> join blocking owner
```

The child command line carries only its two inherited numeric handle values.
Plugin identity, capabilities, limits, source, and reloads cross the framed
channel after containment attestation. The child receives no ambient standard
handles and a generated four-entry environment: `LOCALAPPDATA`, `SystemRoot`,
`TEMP`, and `TMP`. Windows supplies the AppContainer profile and system paths;
the desktop process environment is never inherited.

## Type generation

`PluginContext` and its closed value types are ordinary `mlua::UserData` and
`FromLua` implementations. `E:\Projects\mlua-typegen` extracts the complete
LuaCATS snapshot from these registrations. The generated file is a committed
developer artifact; `build.rs` does not recursively invoke Cargo, and runtime
startup performs no reflection or generation.

The five logging methods intentionally avoid a free-form level string. Invalid
severity names are absent from the generated API instead of becoming runtime
validation branches or a hand-maintained union mapping.

## Proof obligations

- A useful lifecycle script can emit a structured log through the granted port.
- Removing the log capability produces a typed denial and no broker call.
- Every ambient authority name is absent from the script environment.
- Binary Lua chunks are rejected before VM execution and text mode is forced.
- An infinite loop terminates at the instruction budget with JIT disabled.
- Allocations beyond the configured VM ceiling fail without poisoning a new VM.
- LuaCATS generation reflects the Rust registration and contains no
  hand-maintained shadow API.
- The native LPAC probe proves token identity, zero ambient capabilities,
  mitigation policy, Job containment, and deterministic termination.
- The broker integration proves exact handle allowlisting, per-extension
  identity, bounded framing, bounded structured logs, transactional reload,
  and cancellation-safe owner admission.
