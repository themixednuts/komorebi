# Pinned nightly and SIMD value spike

## Decision

Keep the application on stable Rust. Do not adopt the pinned nightly, `-Znext-solver`, nightly portable SIMD, or `-Zbuild-std`.

The stable scalar particle kernel already compiles to 256-bit AVX instructions on the target machine. Explicit AVX2 produced only a small, noisy median improvement and worse tail latency in two of three rotated rounds. Portable SIMD did not reliably beat the scalar source. More importantly, the pinned nightly failed valid builds in the planned GPUI, Drizzle, mlua/LuaJIT, and Windows dependency fixture, while the next-solver arm also crashed during a clean repository check.

This result does not reject GPU acceleration. Decoration effects remain GPU-first in the one GPUI/D3D11 scene. The stable scalar kernel is the clean CPU effect fallback when GPU compute is disabled or unavailable. See [GPU-FOLLOWUP.md](GPU-FOLLOWUP.md).

## Artifacts

- `compiler-measurements.json`: complete stable, nightly-default, and nightly-next-solver compiler matrix.
- `kernel-measurements.json`: candidate profiling, three rotated benchmark rounds, allocations, checksums, and idle CPU.
- `assembly-inspection.json`: stable scalar and nightly portable-SIMD instruction inspection.
- `build-std-measurement.json`: separate `-Zbuild-std` result.
- `diagnostics/*.jsonl`: the same intentionally invalid trait program under all compiler arms.
- `kernel/`: scalar reference, checked AVX2 adapter, portable-SIMD spike, and property tests.
- `compatibility/`: process-aligned Drizzle, mlua/LuaJIT, GPUI Components, and `windows-sys` fixture.
- `runner/`: cancel-safe compiler-matrix runner with atomic native-path checkpoints.

Generated `target/` and `targets/` directories are intentionally ignored.

## Reproduction

Run from this directory. The prototype-local toolchain file pins `nightly-2026-08-27`; the repository root remains stable.

```powershell
cargo +stable test -p particle-kernel --locked
cargo +nightly-2026-08-27 test -p particle-kernel --features portable-simd --locked
cargo +stable run --release -p particle-benchmark --locked -- scalar
cargo +stable run --release -p particle-benchmark --locked -- avx2
cargo +nightly-2026-08-27 run --release -p particle-benchmark --features portable-simd --locked -- portable
```

The full compiler matrix is intentionally expensive and writes a checkpoint after every scope:

```powershell
cargo +stable run --release -p measurement-runner --locked
```

The LuaJIT extension-host fixture uses a static C runtime because the vendored `luajit-src` MSVC build selects `/MT`:

```powershell
$env:RUSTFLAGS = '-Ctarget-feature=+crt-static'
cargo +stable test -p toolchain-extension-compatibility --locked
Remove-Item Env:RUSTFLAGS
```

## Rollback

No production rollback is required: production never left stable. To remove the experiment, delete this prototype directory and the date-pinned nightly with `rustup toolchain uninstall nightly-2026-08-27`. No source, configuration, database, or user-facing state depends on it.
