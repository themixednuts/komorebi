# QuickJS/TypeScript plugin spike

## Decision

Use QuickJS-NG through `rquickjs` plus embedded Oxc for user automation scripts, subject to an out-of-process LPAC proof before production integration. Do not require a `tsconfig.json` or bundler. Generate `komorebi.d.ts`, accept plugin-local relative ESM, and expose only capability-scoped `komorebi:*` modules.

If the sandbox gate fails, retain LuaJIT with JIT disabled. Do not ship LuaJIT's JIT mode: it has little benefit for host-call-heavy plugins, grows the working set, and conflicts with a strong Windows dynamic-code policy.

QuickJS wins the product criterion. TypeScript gives users familiar syntax, native editor inference, static host capability names, top-level `await`, and modern ESM. LuaJIT is decisively faster and smaller, but its authoring and typed API experience is worse.

This is not approval for in-process untrusted extensions. The script worker must remain a brokered, capability-limited process because either native C engine can crash or corrupt its containing process.

## Proven authoring model

- `.ts` and `.mts` are parsed and stripped by embedded Oxc at load time. No Node.js, external executable, `tsconfig.json`, or build step is needed.
- Oxc reports syntax/transform errors but does not perform TypeScript semantic type checking. Editors and optional CI provide type checking.
- `komorebi.d.ts` is generated from the same Rust-owned capability vocabulary. The spike's test checks the exact declaration. Production should generate the complete file at installation/update time and validate VS Code inferred-project discovery.
- Static and dynamic relative ESM imports work. Extensionless imports resolve `.ts`, `.mts`, `.js`, `.mjs`, then `index.ts`. Top-level `await` and a real Tokio-backed async host call work.
- Bare package imports, CommonJS `require`, network resolution, JSON imports, import maps, and root escapes are rejected. The only virtual module is `komorebi:host`.
- Windows module identities are lossless hex-encoded UTF-16. A real NTFS test executes a plugin below a directory containing an unpaired surrogate. Diagnostics decode that identity for display.
- Oxc source maps remap a thrown QuickJS stack back to the original `.ts` path and line. The benchmark separately records transform and invalid-source diagnostic costs.

The dependency policy should stay narrow: first-party `komorebi:*` capabilities and plugin-local relative ESM only. If third-party packages are added later, install a locked, vendored plugin tree out of process; never provide npm/network resolution at runtime.

A bundler is not required. The custom resolver loads a two-module plugin directly, and hot reload replaces the runtime so no stale module cache survives. Bundling can remain an optional packaging optimization after profiling large real plugins. QuickJS bytecode bundles are engine-version-coupled and should not become the user-authored source format.

Runtime transpilation is appropriate for small event-driven scripts: the measured two-module transform is about 0.49 ms, and the whole cold reload is about 1.17 ms. Production should cache generated JavaScript/source maps by source content hash, while treating source as the authority.

## Typed call stack

```text
CLI/plugin broker
  -> HostConfig + PluginRequest
  -> PluginHost<Unconfigured>::configure
       canonicalize root + make resource policy valid
  -> PluginHost<Ready>::execute
       canonicalize/confine entry
       -> AsyncRuntime (memory, stack, deadline, cancellation)
       -> PluginResolver (capability or confined relative module)
       -> PluginLoader
            Oxc parse -> semantic pass -> TS strip -> JS + source map
       -> QuickJS ESM import/top-level await
       -> komorebi:host::focus
       -> Tokio async Rust capability
       -> Vec<HostAction>
       -> ExecutionReport
```

Invalid root state cannot call `execute`. Resolver, transform, runtime, timeout, cancellation, and memory failures return typed Rust errors at their owning boundary. No compatibility API is retained.

The benchmark path is independently auditable:

```text
bench CLI -> Latin-rotated child worker per engine/round
  -> correctness proof
  -> identical fixture stages and black-box outputs
  -> Windows process working-set sample
  -> raw BenchmarkResult JSON
```

## Fair benchmark

Command:

```powershell
cargo run --release -- bench --output results/benchmark.json --rounds 3 --warmup 500 --samples 2000 --loop-iterations 100000 --reloads 50
```

Environment: Windows x86-64, 24 logical CPUs. Every one of the nine measurements ran in a fresh release child process pinned to CPU 0 at above-normal priority. The order uses a three-engine Latin rotation. Each mode gets 500 unmeasured calls, 2,000 raw warm-call samples per round, 100,000-iteration loops, 50 reloads per round, and 16 live empty instances for incremental memory. Outputs are consumed through black boxes and validated first: every engine returns checksum `57053`, actions `left/right/left/right/left`, and snapshot `10`.

The TypeScript and Lua fixtures implement the same stateful scoring plugin, separate scoring module, conditional host action, pure arithmetic loop, host-call loop, and snapshot/restore protocol. They are idiomatic and unminified.

All times below are medians across three process runs, except warm and reload percentiles which aggregate the raw samples from those runs.

| Measurement | QuickJS + TS | LuaJIT, JIT off | LuaJIT, JIT on |
|---|---:|---:|---:|
| Runtime creation | 44.6 us | 98.4 us | 70.0 us |
| Context creation | 252.4 us | included | included |
| Read two source files | 276.1 us | 278.7 us | 241.2 us |
| TS transform + source maps | 486.3 us | n/a | n/a |
| Invalid-source diagnostic | 41.9 us | 14.2 us | 13.4 us |
| Compile two modules | 342.3 us | 35.0 us | 34.2 us |
| Instantiate/import plugin | 193.5 us | 4.6 us | 5.4 us |
| First invocation | 4.1 us | 2.3 us | 2.0 us |
| Warm invocation p50 | 0.8 us | 0.4 us | 0.3 us |
| Warm invocation p95 | 1.4 us | 0.8 us | 0.7 us |
| Warm invocation p99 | 1.5 us | 2.8 us | 2.4 us |
| Pure script, 100k iterations | 10.771 ms | 1.454 ms | 0.650 ms |
| Rust host calls, 100k | 24.153 ms | 6.816 ms | 6.769 ms |
| Hot reload p50, including state handoff | 1.171 ms | 0.133 ms | 0.148 ms |
| Hot reload p95 | 1.930 ms | 0.196 ms | 0.233 ms |
| Teardown | 183.3 us | 120.0 us | 141.7 us |
| Loaded process working-set delta | 128 KiB | 104 KiB | 292 KiB |
| Incremental empty runtime/context | 125 KiB | 57 KiB | 57 KiB |
| Median working-set change after 50 reloads | -16 KiB | +44 KiB | +16 KiB |

Authored size: TypeScript is 44 lines/1,114 bytes; Lua is 45 lines/930 bytes. The QuickJS Rust host/resolver/path/transpiler glue is 744 lines/22,972 bytes. This figure is deliberately conservative and not a production estimate; it includes the full spike execution host and diagnostic plumbing, but excludes the benchmark harness.

JIT-off LuaJIT is about 7.4 times faster in the pure loop, 3.5 times faster in the host-call loop, and 8.8 times faster to reload than QuickJS. Turning JIT on roughly doubles pure-script throughput but does not materially improve the host-call workload. For window-manager automation, warm call and reload latency remain small enough that TypeScript DX dominates; CPU-heavy extensions should not run on the window-manager control path in either language.

`WorkingSetSize` from `K32GetProcessMemoryInfo` is a process resident-set snapshot, not private committed bytes. Allocator reuse, DLL sharing, paging, GC timing, and the small deltas make it noisy. The negative QuickJS reload delta is evidence of that noise, not proof of a leak or memory recovery. Repeated reload did not show monotonic growth in 50 cycles, but this is not a leak proof. Production validation should use a long-running LPAC worker plus private bytes/ETW and thousands of reloads.

The benchmark's hot host function is deliberately synchronous for both engines so it measures the same Rust/script crossing. The production `PluginHost` exposes an async Promise and proves Tokio integration separately. Benchmark throughput is `iterations / elapsed`; the raw JSON retains every latency and stage sample so alternate statistics can be computed without rerunning.

## Reload, errors, cancellation, and async

- Reload creates a new runtime, explicitly snapshots old state, restores it into the new runtime, then drops the old runtime. This invalidates the complete ESM cache and avoids partial graph invalidation rules. State is data, not live JS objects.
- TypeScript stack locations are source-mapped. Loader errors name the denied capability/import policy. Lua errors are simpler because there is no transform layer.
- QuickJS memory, maximum stack, elapsed deadline, and explicit cancellation paths are configured at runtime. Tests prove memory rejection, an infinite-loop deadline, and preemptive cancellation.
- CPU-bound JavaScript cancellation is preemptive through QuickJS's interrupt hook. An in-flight Rust future still needs cooperative cancellation; `tokio::time::timeout` alone cannot stop native or synchronous work.
- `rquickjs::AsyncRuntime` serializes access behind its runtime lock. The production shape should be one owner runtime in one worker process with an async message bridge, not a shared general-purpose runtime used directly from arbitrary control-path tasks.

## Sandbox assessment

QuickJS is an interpreter and does not need writable/executable JIT pages. LuaJIT with JIT enabled does. Windows `ProcessDynamicCodePolicy` can prevent generating or modifying executable code, so QuickJS and LuaJIT JIT-off are compatible with a stronger mitigation profile in principle.

LPAC denies resources unless capabilities/DACLs grant them. The spike exposes no filesystem, network, registry, COM, process, or window globals to JavaScript; only the typed host capability exists. The module loader confines reads to the canonical plugin root.

This remains an unproven gate because the executable was not launched as LPAC. A production decision needs a packaged worker test that applies LPAC, dynamic-code prohibition, child-process denial, read-only plugin ACLs, no network capability, brokered IPC, crash recovery, and update/signing behavior. Do this before integrating the engine into komorebi.

## Binding and maintenance choice

`rquickjs` is the best fit among current Rust bindings:

| Binding | Assessment |
|---|---|
| `rquickjs` 0.12.2 | Chosen. Direct QuickJS-NG wrapper with custom loaders, async runtime, interrupt, memory/stack limits, jobs, and shipped MSVC bindings. Its Windows MSVC support is explicitly experimental. |
| `quickjs_runtime` 0.17.3 | Serious alternative with its own worker event loop and TypeScript feature. It is a much larger, opinionated facade; adopting it would duplicate the broker/owner architecture and reduce control over boundaries. |
| `quick-js` 0.4.1 | Not suitable. Older, thinner API and Windows guidance centered on the GNU/MSYS2 toolchain rather than this MSVC application. |
| `quickjs-rusty` 0.14 | Not selected. Simpler embedding surface, but it lacks the demonstrated async/module/resource-control depth needed here. |

QuickJS-NG and all four considered Rust bindings use permissive licensing; the chosen stack (`rquickjs`, QuickJS-NG, Oxc) is MIT. Legal still needs the normal transitive-license inventory.

Maintenance is the larger risk. QuickJS-NG is active, but `rquickjs` 0.12.2 lags the current QuickJS-NG release, Windows MSVC is marked experimental, and open upstream work includes an NG update, async refactor, and a context-drop deadlock fix. Pin exact versions, exercise Windows x64 in CI, fuzz the module/host boundary, and define an owned upgrade cadence before shipping.

The local `.cargo/config.toml` makes all MSVC objects use the static CRT. Without one consistent CRT choice, the vendored LuaJIT and Rust/QuickJS link produced `LNK4098`; the configured build and test are clean. Production must choose one CRT policy for every vendored C dependency rather than suppressing that warning.

## Sources

- [`rquickjs` repository and platform/support matrix](https://github.com/DelSkayn/rquickjs)
- [`rquickjs` runtime limits, interrupt, jobs, and loader API](https://docs.rs/rquickjs/latest/rquickjs/runtime/struct.Runtime.html)
- [`rquickjs` loader API](https://docs.rs/rquickjs/latest/rquickjs/loader/)
- [QuickJS-NG repository, releases, and MIT license](https://github.com/quickjs-ng/quickjs)
- [`quickjs_runtime` architecture](https://docs.rs/quickjs_runtime/latest/quickjs_runtime/)
- [LuaJIT command-line/JIT controls](https://luajit.org/running.html)
- [Microsoft AppContainer and LPAC overview](https://learn.microsoft.com/en-us/windows/win32/secauthz/implementing-an-appcontainer)
- [Microsoft dynamic-code process policy](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-process_mitigation_dynamic_code_policy)
