# Integer ranges and loop optimization

Release builds use LLVM O3 and the selected CPU's cost model. These optimizations
apply to both JIT execution and standalone applications, not just `heavy.nerv`.

## Range proofs

The backend tracks inclusive i64 intervals for constants, unreassigned locals,
counted-loop indices, and simple arithmetic. A whole-function write scan includes
branches and expressions: declaring a binding `mut` is harmless when it is never
reassigned, but any reassignment prevents treating its initial value as invariant.
Array element mutations do not change a fixed-length array's header.

Arithmetic intervals are discarded when an endpoint could overflow; wrapping
arithmetic is never treated as monotonic. Calls and mutable array elements do not
produce trusted intervals. Signed and unsigned language semantics do not change.

Known allocation lengths propagate through unreassigned handle aliases. When an
index's entire interval lies inside that length, its bounds comparison can be
omitted. Unknown indices, reassigned handles, negative indices, and potentially
wrapping expressions retain runtime checks. Null-handle checks remain. Division
uses range proofs to avoid redundant u32 fit tests, otherwise guarding the fast
path dynamically. This is conservative interval analysis, not full path-sensitive
or interprocedural analysis.

## Reused divisors

In release counted loops with a proven minimum of 16 iterations, an unreassigned
local divisor defined outside the loop is eligible for reciprocal division. For
nonnegative u32-sized operands and a positive divisor:

1. Precompute `m = floor(2^32 / d)`.
2. Estimate `q = (n * m) >> 32` using a widening multiply.
3. Compute `r = n - q*d`; if `r >= d`, increment `q` and subtract `d` from `r`.

The reciprocal's rounding error makes the estimate low by at most one because
`n < 2^32`. A special case for `d == 1` allows a u32 multiplier representation.
All results are exact integer results, not floating-point approximations. LLVM
hoists the setup when legal; no expression or check is manually moved ahead of
source-level effects or zero-trip loops. Short/unknown-trip loops and changing
divisors keep the existing hardware-division lowering.

Negative or wider operands use signed i64 fallback. Division by zero and
`i64::MIN / -1` / `% -1` retain their existing behavior.

### Correction-free reciprocal for bounded numerators

When interval analysis also proves `0 <= n <= B <= u32::MAX`, the compiler tries
`m = ceil(2^32/d)` with `e = m*d - 2^32`. If `B*e < 2^32`, then
`floor(n*m/2^32)` is the exact quotient without per-iteration correction. The
rounding error is `n*e/(d*2^32) < 1/d`, too small to cross an integer boundary in
`n/d`. The error-bound product fits u64 for every eligible divisor and bound.

The eligibility predicate depends only on the invariant divisor and the proven
bound, so LLVM can move it outside the loop. Failed proofs use the corrected
reciprocal path above; unknown numerator ranges use that path directly. A
dedicated divisor-1 case preserves the full `2^32` multiplier's semantics when
using narrow arithmetic. This applies to general bounded numerators, not a
special case for divisor 22 or a specific benchmark file.

## Vectorization

Removing proven bounds checks exposes fixed-length map, reduction, dot-product,
and stencil loops to LLVM's existing vectorizer. We do not assert array `noalias`,
force vector widths, or enable unsafe floating-point reassociation. Aliases and
target costs can still prevent SIMD. The current dynamic-divisor `heavy.nerv`
hot path remains scalar (unrolled by two); its speedup is from reused reciprocal
arithmetic and range proofs, not a claim of SIMD execution.

Regression tests compare debug/release and JIT/standalone results, check emitted
vector IR for map/reduction workloads on ARM64/x86-64, and exercise u32 boundaries,
signed fallbacks, deterministic randomized operands, handle rebinding, inclusive
ranges, zero-trip loops, and overflow. See [benchmark results](../time-release/RESULTS.md)
for measured standalone timings; no universal speedup is implied.
