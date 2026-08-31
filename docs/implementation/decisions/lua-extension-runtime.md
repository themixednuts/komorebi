# Lua extension runtime decision

## Decision

Production extensions use Rust, `mlua`, and LuaJIT. QuickJS is not retained as
a second runtime or compatibility path. Extension APIs are registered once as
typed Rust values and functions; scripts never construct transport messages,
open filesystem or network handles, or load native modules directly.

LuaLS/EmmyLua metadata is generated from the real `mlua` registrations with
the `mlua-typegen` tool in `E:\Projects\mlua-typegen`. Hand-maintained shadow
declarations are not authoritative. The integration will run
`cargo mlua-typegen` for the extension-host crate, commit or package its one
complete generated snapshot, and use explicit mappings only where native Rust
types do not prove their Lua representation.

## JIT containment gate

JIT-on is preferred, but only inside the per-extension LPAC worker and only if
the containment spike proves all of the following together:

- executable-code allocation is confined to that worker and cannot be used to
  load DLLs, enable LuaJIT FFI, or escape brokered capabilities;
- `package.cpath`, native module searchers, ambient process launch, filesystem,
  registry, network, COM, and Win32 access are absent;
- memory, callback time, instruction/preemption, broker-request, and output
  limits remain enforceable while traces are compiled;
- worker termination and hot reload cannot leave an accepted broker operation
  unowned or partially applied;
- Windows exploit mitigations compatible with LuaJIT remain enabled, and the
  exact mitigation relaxation needed for generated code is documented and
  tested rather than broadly disabled;
- malicious-script tests cannot turn writable data into executable code outside
  LuaJIT's allocator or reuse broker handles across extension identities.

Windows `ProhibitDynamicCode` and LuaJIT trace compilation are mutually opposed
inside one process. The spike therefore compares two explicit worker profiles:

1. `JitEnabled`: LPAC plus the strongest compatible mitigations and a narrowly
   justified dynamic-code allowance.
2. `JitDisabled`: LuaJIT interpreter mode plus `ProhibitDynamicCode`.

There is no silent runtime fallback. The host selects a proven profile when it
constructs the worker, records that profile in diagnostics, and never changes
it inside a running extension. If the `JitEnabled` profile fails any containment
proof, production uses `JitDisabled` until the design changes and the complete
gate is rerun.

## Type-generation contract

- Rust `UserData`, module tables, async functions, enums, documentation, and
  return values are the source of truth.
- `mlua-typegen` emits the LuaCATS snapshot consumed by LuaLS and EmmyLua.
- Broker capabilities, lifecycle contexts, cancellation, events, action IDs,
  and structured errors must appear in that generated surface.
- Dynamic values require an explicit `mlua-typegen.toml` mapping with a reason;
  `any` is not used to bypass an unmodeled interface.
- Generation is a build/CI developer step, not runtime reflection and not a
  recursive Cargo invocation from the extension host's `build.rs`.

