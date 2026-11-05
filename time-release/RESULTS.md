# Local standalone benchmark — 2026-11-04

## Bounded, correction-free reciprocal — 2026-11-05

Same Apple M3 Pro, native-CPU release executables, unchanged full billion-iteration workload. One discarded warmup per language, five alternating measured rounds, validated outputs, and no concurrent validation jobs during timing.

| Round | Nerv standalone (ms) | Rust release (ms) |
| --- | ---: | ---: |
| 1 | 294.336 | 508.592 |
| 2 | 294.223 | 508.482 |
| 3 | 293.373 | 508.967 |
| 4 | 293.801 | 509.318 |
| 5 | 293.932 | 508.640 |
| **Median** | **293.932** | **508.640** |

Nerv/Rust median ratio: **0.578×**. Nerv took **42.2% less time**, a Rust/Nerv speed ratio of **1.730×** for this benchmark. Nerv's median is approximately **28.1% lower** than the preceding 408.919 ms result. These results are machine/workload-specific, not a universal language ranking.

An additional proof checks whether a rounded-up reciprocal is exact for the numerator's known upper bound. Eligible loops omit the per-iteration correction; other operands keep the previous fallbacks. Disassembly confirms the setup predicate precedes both loops, with 10,000 outer rows and the two-way-unrolled 100,000-iteration inner loop intact. The selected divisor-22 hot loop is still scalar; no forced SIMD or workload shortcut was used.

Narrowing induction-variable storage did not produce a measured improvement and was not retained. LLVM vectorization remarks reported the existing target cost model prefers scalar interleaving for this loop. See [the exactness proof](../docs/optimization.md#correction-free-reciprocal-for-bounded-numerators).

```sh
./tools/time-release.sh --cpu native --runs 5 --warmups 1 --json-output target/benchmarks/nested-loops-bounded-reciprocal.json
```

Raw report: `target/benchmarks/nested-loops-bounded-reciprocal.json` (ignored build output).

Validation: **152 compiler/workspace tests**, **12 strict-provenance Miri checks**, and **6 benchmark-helper tests** passed. New regressions exercise both sides of the reciprocal error-bound predicate (including equality), divisor 1, u32 boundaries, signed/wide fallbacks, and output ordering before division-by-zero failure.

## Range proofs and reciprocal division — 2026-11-05

Same Apple M3 Pro, native-CPU standalone release builds, unchanged billion-iteration sources and Rust optimizer barriers. One warmup per language followed by five alternating measured rounds; all outputs validated. No concurrent validation jobs ran during timing.

| Round | Nerv standalone (ms) | Rust release (ms) |
| --- | ---: | ---: |
| 1 | 411.429 | 508.628 |
| 2 | 408.107 | 509.246 |
| 3 | 409.111 | 508.507 |
| 4 | 408.919 | 509.979 |
| 5 | 408.375 | 508.655 |
| **Median** | **408.919** | **508.655** |

**Nerv / Rust median ratio: 0.804×**. Nerv took **19.6% less time** (Rust/Nerv speed ratio **1.244×**) on this workload. This is one machine/run, not a general language ranking. Relative to the preceding Nerv median of 515.202 ms, the new median is about 20.6% lower; the matched Rust samples above are the fair current comparison.

Disassembly confirms all 10,000 rows and 100,000 inner iterations per row remain. One reciprocal setup division runs before both loops; the selected hot loop uses multiply/shift/correction, not per-iteration division. Range proofs remove checks for the known row interval; the final random index remains checked. The wide fallback retains signed division. The divisor-22 hot path is still scalar and unrolled by two, not SIMD.

Separate map/reduction tests confirm vector IR, and dot-product/stencil/zero-trip fixtures match JIT and standalone in debug and release. Full suite: **150 passed**. See [optimization details](../docs/optimization.md) for eligibility and limits.

```sh
./tools/time-release.sh --cpu native --runs 5 --warmups 1 --json-output target/benchmarks/nested-loops-reciprocal.json
```

Raw samples and build metadata: `target/benchmarks/nested-loops-reciprocal.json` (ignored build output). PowerShell execution remains unverified locally.

## Optimized native-CPU comparison — 2026-11-05

Same Apple M3 Pro and workload; both standalone release programs now target the native CPU. One warmup, five alternating measured rounds; all outputs validated. No concurrent validation jobs ran during timing.

| Round | Nerv standalone (ms) | Rust release (ms) |
| --- | ---: | ---: |
| 1 | 515.202 | 515.709 |
| 2 | 603.029 | 521.147 |
| 3 | 513.913 | 521.531 |
| 4 | 509.267 | 524.968 |
| 5 | 515.323 | 514.075 |
| **Median** | **515.202** | **521.147** |
| Mean | 531.347 | 519.486 |

Median ratio **0.989×**: Nerv's median was about **1.1% lower**, but its mean was higher due to the second sample. Treat this as **roughly equal performance**, not evidence of a consistent Rust win or Nerv win. Compared with the original Nerv median below, Nerv is approximately **2.92× faster** (CPU tuning also changed).

Nerv now inlines checked array accesses and uses guarded u32 division/remainder with signed i64 fallback. Disassembly retains all 10,000 rows and 100,000 inner iterations per row. The original benchmark sources and Rust optimizer barriers remain unchanged by this optimization.

```sh
./tools/time-release.sh --cpu native --runs 5 --warmups 1 --json-output target/benchmarks/nested-loops-native.json
```

Raw report: `target/benchmarks/nested-loops-native.json` (ignored build output). Runtime ABI v2 is required for the optimized objects. Full compiler tests and strict-provenance Miri passed; PowerShell execution remains unverified locally.

## Original pre-optimization comparison

- **Machine:** Apple M3 Pro, ARM64, macOS 26.6.2.
- **Toolchain:** rustc 1.99.1 (`58cf8f9c3`), LLVM 23.1.3; no custom `RUSTFLAGS`.
- **Workload:** 10,000 × 100,000 = **1,000,000,000 inner iterations per invocation**.
- Both programs are standalone release executables. Compilation is excluded.
- One discarded warmup per language; five measured rounds with alternating order.
- Every invocation passed the numeric result validation.

| Round | Nerv standalone (ms) | Rust release (ms) |
| --- | ---: | ---: |
| 1 | 1502.910 | 509.352 |
| 2 | 1503.564 | 508.517 |
| 3 | 1515.323 | 531.240 |
| 4 | 1503.570 | 508.520 |
| 5 | 1503.822 | 509.365 |
| **Median** | **1503.570** | **509.352** |
| Mean | 1505.838 | 513.399 |

**Nerv / Rust median ratio: 2.952×.** Nerv took approximately **2.95× as long** as Rust for this workload on this machine.

The timed Nerv file was `target/release/heavy-nerv`, built with `nerv build ... --release`; it was invoked without arguments, not through the compiler/JIT. Rust was `time-release/target/release/heavy`.

Disassembly confirms the loop work remains in both binaries: Nerv has 10,000 outer iterations and 100,000 scalar divisions per row. Rust has 10,000 rows and an inner loop unrolled by two, still totaling 100,000 scalar divisions per row. Nerv makes two checked `nerv_array_i64_at` calls per inner iteration; Rust uses register accumulators without those calls. These are observed implementation differences, not a general language ranking.

Reproduce:

```sh
./tools/time-release.sh --runs 5 --warmups 1 --json-output target/benchmarks/nested-loops.json
```

The full local JSON report, including raw samples and toolchain/platform metadata, is in `target/benchmarks/nested-loops.json` (ignored build output). See [methodology](README.md). CPU frequency and other OS activity are not pinned; timings vary between runs and machines. Windows tooling was updated but not executed locally.
