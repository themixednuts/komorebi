# QuickJS plugin-host spike

This throwaway workspace evaluates embedded TypeScript/JavaScript against the existing LuaJIT direction. It does not change komorebi production code.

## Reproduce

```powershell
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
cargo run --release -- bench --output results/benchmark.json --rounds 3 --warmup 500 --samples 2000 --loop-iterations 100000 --reloads 50
cargo run -- types --output komorebi.d.ts
cargo run -- run fixtures/typescript/plugin.ts --root fixtures/typescript
```

The benchmark command starts a fresh child process for every engine/round and writes all raw samples to JSON. See [SPIKE.md](SPIKE.md) for the result and recommendation.
