# Thermo-nuclear code-quality audit

## Verdict

Pass for a disposable decision prototype. Do not merge the experimental AVX2 or portable-SIMD kernels into production. The measured decision is deletion: retain the existing stable toolchain and a simple scalar kernel, then build GPU acceleration behind the already chosen decoration-scene primitive.

## Call-stack and ownership audit

- The runner has one binary entry, one Tokio runtime, one owner per child, and no `block_on` or nested executor.
- Matrix policy, child-process ownership, durable report publication, and top-level orchestration live in separate focused modules; the audit removed the original 532-line all-in-one runner.
- Cancellation owns child termination through `kill_on_drop`; a cancelled operation cannot publish a fabricated complete report.
- Measurement state has one writer. Atomic replacement is concentrated at the filesystem boundary and uses native wide paths.
- The compatibility fixture is split by real process ownership. GPUI, LuaJIT, and state persistence are not collapsed into an artificial giant executable.
- `ParticleStep` rejects non-finite and invalid ranges at construction. `ParticleBatch` hides its component vectors and preserves equal lengths.
- The scalar loop is the reference implementation. AVX2 unsafe code is narrow, capability-gated once, and followed by a scalar tail. Portable SIMD is feature-gated to the pinned nightly.
- The benchmark checks semantic equivalence and allocation count before interpreting timing. Its candidate is profile-selected rather than assumed.
- Idle measurement parks on a one-shot deadline; there is no continuous state poll.
- GPU follow-up preserves a single GPUI scene, device owner, presentation path, and typed effect vocabulary.

## Complexity audit

No production service hierarchy, dynamic backend registry, renderer facade, compiler abstraction, or generalized benchmark framework is proposed. The experimental variants are local to the prototype. The production result removes rather than adds nightly flags, unsafe code, and compiler coupling.

Conditional growth is bounded to closed backend admission and device-recovery state. Hardware, CPU-update, WARP-scene, disabled, and unavailable are domain states, not scattered booleans. Lua and callers do not branch on renderer details.

## Known constraints

- One machine and one complete compiler trial limit timing generalization. Hard compiler/link failures still make the adoption decision conclusive.
- Benchmark tail latency is noisy. That weakens any performance-adoption claim and therefore supports the simpler scalar choice.
- Stable repository-wide strict Clippy currently fails in pre-existing code outside this prototype; the prototype workspace itself passes strict Clippy.
- Vendored LuaJIT's MSVC runtime selection requires a consistent static-CRT extension-host build policy.
- WARP, device loss, GPUI renderer extension, and GPU compute are not proven by this spike. The design labels them as required follow-up evidence.
- VectorWare GPU SIMD and `rust-gpu` are not production dependencies. Revisit only when a consumable, supported toolchain can satisfy the same closed effect contract.

## Rust-beyond-the-type-system checks

Following the failure classes in [Bugs Rust Won't Catch](https://corrode.dev/blog/bugs-rust-wont-catch/), the audit explicitly checked cancellation, subprocess lifetime, partial durable writes, native path fidelity, target-feature admission, unsafe preconditions, third-party CRT consistency, numerical equivalence, allocation behavior, idle CPU, and disk exhaustion. These are runtime and integration properties; type correctness alone would not prove them.

## Release gates

- `cargo +stable fmt --all --check`
- stable and nightly property tests for every experimental kernel
- stable strict Clippy over the prototype workspace
- machine-readable report schema and `complete: true`
- identical checksums and zero timed allocations across kernel arms
- `git diff --check`
- no production `rust-toolchain.toml` modification
