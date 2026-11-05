# Rust loop baseline

Adapted from [bddicken/languages' Rust loops benchmark](https://github.com/bddicken/languages/tree/main/loops/rust/src).

This standalone crate matches `example/heavy.nerv`: **10,000 outer × 100,000 inner = 1,000,000,000 logical inner iterations per invocation**, a divisor of 22, and a random value in `0..10_000`. It uses `i64` arithmetic to match Nerv. Result values need not match between processes because of the random addend.

Build and compare both **standalone release executables** from the repo root:

```sh
./tools/time-release.sh --runs 5 --warmups 1 --json-output target/benchmarks/nested-loops.json
```

```powershell
.\tools\time-release.ps1 -Runs 5 -Warmups 1 -JsonOutput target\benchmarks\nested-loops.json
```

See the [recorded Apple M3 Pro comparison](RESULTS.md) for one measured run and its raw sample table.

To run just the Rust baseline:

```sh
cargo run --release --manifest-path time-release/Cargo.toml
```

An optional positional argument overrides the Rust divisor. The comparison scripts use its default of 22. Release uses opt-level 3, LTO, and one codegen unit.

## What is timed

The scripts first build `target/release/nerv`, then run `nerv build example/heavy.nerv --release -o target/release/heavy-nerv` (`.exe` on Windows). They build the Rust executable at `time-release/target/release/heavy`. **Only these two generated benchmark executables are timed**, with no source-file arguments, `--run`, JIT initialization, Cargo, or compiler work in the timed commands.

One warmup invocation of each program is discarded by default. Measured rounds alternate Nerv-first and Rust-first order. Reports include individual samples, minimum, median, mean, maximum, and the **Nerv median / Rust median** ratio. JSON records raw timings, binary paths, CPU/platform, benchmark settings, and toolchain versions when building (unknown for prebuilt binaries). Each invocation performs the full workload, including warmups; five samples plus one warmup means six billion logical iterations per language.

The timed interval is complete process execution, including process startup, array initialization, checks, and output—not an isolated inner-loop timer. Output is captured outside the terminal and validated: the sum must be in `1,049,940..1,059,940`, and Nerv must additionally print its void-main result `0`. Validation occurs after the timer stops. LLVM library-path variables are removed from child environments.

## Optimization and fairness

Both languages obtain an opaque runtime divisor equal to 22. Nerv uses `nerv.random(1) + 22`; Rust uses `black_box`. This avoids giving only Nerv constant-divisor specialization while forcing dynamic division in Rust. Rust adds one compiler barrier **per outer row**, not per inner iteration, and exposes the full array to `black_box` to prevent dropping unobserved rows or hoisting one inner sum across all rows.

These are logical source-level iteration counts, not a claim of one billion identical machine instructions: optimization is allowed. Nerv now inlines checked array accesses; LLVM can hoist checks and keep accumulators in registers. Integer division/remainder uses a guarded u32 fast path when both operands fit, with signed i64 fallback. Safety checks and signed overflow behavior are preserved. These implementation differences are part of this benchmark, not a general language-performance verdict.

CPU settings match: scripts default to `--cpu generic` / `-Cpu generic` for both languages. Use `--cpu native` / `-Cpu native` to match JIT host tuning; Rust receives `-C target-cpu=native`. Native binaries may not run on older CPUs. JSON records the selected CPU and effective Rust flags; these are unknown with `--skip-build`. Avoid custom `RUSTFLAGS` when comparing, and avoid other CPU-heavy work during measurements. `CARGO_ENCODED_RUSTFLAGS` must be unset because it overrides `RUSTFLAGS`.

```sh
./tools/time-release.sh --cpu native --runs 5 --warmups 1 --json-output target/benchmarks/nested-loops-native.json
```

## Reusing existing binaries

The compiler also applies conservative [range proofs and reused-divisor optimization](../docs/optimization.md). Eligible long counted loops precompute a reciprocal for an invariant divisor and replace each narrow `/` or `%` with exact multiplication/correction. Wider or negative operands retain signed i64 division. No benchmark-specific divisor or source rewrite is used.

```sh
./tools/time-release.sh --skip-build --runs 3 --warmups 1
```

`--skip-build` / `-SkipBuild` does not configure LLVM, invoke Cargo, or require source files. `--nerv-exe-path` / `-NervExePath` now identifies the **standalone Nerv program**, never the Nerv compiler. Custom executable paths assume appropriately built release programs; the tool cannot infer their optimization settings. A custom `--program-path` / `-ProgramPath` is compiled before measurement; ensuring its workload matches a custom Rust baseline is the caller's responsibility.
