# Measurement results

## Environment

- Source revision: `834f3c98b34d36067b18b00aa81eaf2107d1bf2c`
- Host: Windows 11, `x86_64-pc-windows-msvc`
- CPU: AMD Ryzen 9 5900X, 12 cores / 24 logical processors, native target `znver3`
- Stable: Rust 1.97.1, LLVM 22.1.6
- Nightly: `nightly-2026-08-27`, Rust 1.100.0-nightly, LLVM 23.1.0
- Nightly installation: 12,789.8344 ms with Cargo, Clippy, rust-src, rust-std, rustc, and rustfmt

The compiler matrix used one controlled trial. Repeating the full multi-hour matrix could refine timing variance but cannot reverse the correctness failures that disqualify nightly.

## Repository compiler matrix

| Operation | Stable | Nightly default | Nightly next solver |
| --- | ---: | ---: | ---: |
| Clean check | 143,892 ms pass | 101,597 ms pass | 75,077 ms **fail** |
| Incremental check | 2,706 ms pass | 2,728 ms pass | 38,516 ms pass after failed clean arm |
| Debug build | 192,159 ms pass | 177,149 ms pass | 115,771 ms pass |
| Release build | 257,735 ms pass | 250,322 ms pass | 330,467 ms pass |
| Test | 30,181 ms pass | 58,764 ms pass | 66,474 ms pass |
| Strict Clippy | existing-code fail | nightly-lint fail | nightly-lint fail |
| Release executable bytes | 74,316,288 | 75,920,384 | 75,920,384 |

The next-solver clean check terminated with `STATUS_STACK_BUFFER_OVERRUN` while compiling a Windows crate. Its subsequent warmed success is not clean-check evidence. Nightly tests were roughly 95% slower and next-solver tests roughly 120% slower than stable. Nightly release output was about 2.2% larger; next-solver release build was about 28% slower.

Stable strict Clippy found two existing repository issues (`set_keepalive` name collision and iterating map key/value pairs when only values are used). Nightly instead failed earlier on a changed trailing-semicolon macro lint. These failures are recorded, not attributed to the prototype.

## Planned-stack compatibility fixture

| Operation | Stable | Nightly default | Nightly next solver |
| --- | ---: | ---: | ---: |
| Clean check | 186,477 ms pass | 186,664 ms pass | 186,603 ms pass |
| Incremental check | 4,131 ms pass | 3,155 ms pass | 3,413 ms pass |
| Debug build | 196,953 ms pass | 122,042 ms **fail** | 158,700 ms **fail** |
| Release build | 287,733 ms pass | 37,280 ms **fail** | 242,023 ms **fail** |
| Test | 29,713 ms pass | 30,278 ms **fail** | 59,191 ms **fail** |
| Strict Clippy | 8,250 ms pass | 3,758 ms pass | 10,449 ms pass |

The fixture includes Drizzle 0.1.16's query API, generated `build.rs` migration and `SQLiteFromRow` derive, rusqlite, Blob storage, mlua 0.12 with vendored LuaJIT, GPUI Components at `6d07863f`, GPUI/Zed at `797e5dc9`, and `windows-sys`. Nightly build/test arms either crashed rustc with `STATUS_STACK_BUFFER_OVERRUN` or reached the linker with missing generated extension-host inputs. Stable passed the same source and lockfile.

Vendored LuaJIT emits `LNK4098` under Rust's default dynamic CRT because `luajit-src` builds its MSVC static library with `/MT`. Building the isolated extension-host process with `-Ctarget-feature=+crt-static` passes without that warning. This is a required process build policy until the dependency exposes a compatible CRT selection.

## Diagnostic comparison

All three compiler arms rejected the same intentionally invalid iterator program with equivalent primary information: `expected Iter<'_, u16> to yield u32 but yields &u16`. No diagnostic-quality benefit justified the nightly risk.

## Hot-kernel selection

The profiler measured 20,000 frames:

| Candidate | Elapsed |
| --- | ---: |
| Particle update | 20,724,600 ns |
| Geometry transform | 1,457,100 ns |
| Effect parameter transform | 1,167,100 ns |

Particle updates represented about 88% of measured candidate CPU time, so only that kernel advanced to SIMD comparison.

## SIMD comparison

The benchmark used 2,048 particles, 512 iterations per sample, 20 warmups, 120 measured samples, and a rotated order across three rounds. Every backend produced checksum `6133576.603802638`, made zero timed allocations, and consumed zero measured CPU during the two-second event-free idle check.

| Round | Backend | p50 ns | p95 ns | p99 ns | Mean ns |
| ---: | --- | ---: | ---: | ---: | ---: |
| 1 | Scalar/autovectorized | 206,500 | 215,200 | 257,000 | 208,489 |
| 1 | Explicit AVX2 | 194,800 | 202,500 | 221,100 | 196,378 |
| 1 | Portable SIMD | 307,500 | 580,000 | 1,773,200 | 359,597 |
| 2 | Explicit AVX2 | 308,800 | 647,500 | 1,335,500 | 365,914 |
| 2 | Portable SIMD | 299,100 | 469,300 | 650,700 | 313,998 |
| 2 | Scalar/autovectorized | 322,100 | 415,400 | 1,280,100 | 393,256 |
| 3 | Portable SIMD | 312,400 | 1,628,700 | 2,712,900 | 460,529 |
| 3 | Scalar/autovectorized | 336,400 | 403,200 | 1,059,800 | 406,337 |
| 3 | Explicit AVX2 | 298,700 | 416,100 | 1,382,800 | 354,050 |

Explicit AVX2 improved p50 by 4-11% within each round but regressed p95 in two rounds. Portable SIMD was inconsistent and did not reliably beat scalar. Assembly inspection explains the result: stable compiled the scalar loop to `vbroadcastss`, `vmulps`, `vaddps`, and `vmovups` over `ymm` registers, the same relevant 256-bit class observed for nightly portable SIMD.

The correct production choice is the readable stable scalar loop. The benchmark is intentionally sensitive enough to show system noise, so the report does not overstate the small median delta as a durable benefit.

## `-Zbuild-std`

The separate nightly portable-SIMD build completed in about 80 seconds. Its executable was 252,928 bytes versus 231,936 bytes normally, about 9.05% larger, with no established runtime benefit. Do not adopt it.

## Decision and limits

Stable satisfies correctness and dependency compatibility. Nightly fails both adoption gates. Portable SIMD adds unstable surface without a repeatable material win; explicit AVX2 adds unsafe maintenance without a tail-latency win. Production remains stable and introduces no SIMD abstraction.

This spike did not measure GPUI renderer modification, hardware GPU compute, WARP, device loss, laptop power, or non-x86 CPUs. Those belong to the GPU effect backend follow-up. Current GPUI Windows internals use a private D3D11/HLSL renderer; they do not expose a supported public custom-shader API.

