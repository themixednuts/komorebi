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

The current slice proves the in-process VM contract with consumer-owned fake
ports. It is not yet a production trust boundary. Wiring untrusted source into
the desktop host is forbidden; the next slice places this exact VM behind the
LPAC worker/broker process and gives the worker no ambient handles. Once a call
is admitted to that broker, the broker owns it through terminal completion;
dropping UI or plugin result interest cannot split or duplicate its effect.

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
- The later LPAC integration proves token identity, handle allowlisting,
  mitigation policy, per-extension broker identity, termination, and hot reload.
