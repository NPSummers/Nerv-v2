# Execution-region memory and runtime ABI (version 1)

Each JIT invocation or standalone entry owns a thread-local execution region. A test/benchmark batch shares one region. Allocations survive lexical blocks and function returns; region teardown releases every still-owned allocation exactly once. Regions nest in stack order, cannot migrate between threads, and native frees remove allocations from the owning region, including a parent region. Fatal errors print `runtime error: ...` and exit 1 without unwinding through generated code; the OS reclaims memory on fatal exit.

## Representation and lifetime

- Integers are signed 64-bit values, floats are IEEE `double`, and booleans use the target's scalar bool representation.
- Strings and arrays are opaque, pointer-sized handles. Strings own immutable UTF-8, arrays own fixed-length `i64` elements. Generated code never reads Rust `String`/`Vec` layouts; operations go through runtime functions. String producers allocate distinct handles, including empty strings.
- Structs are target-native LLVM aggregates in declaration field order. Nested structs copy by value; string/array fields copy handles and share the allocation. Payload enums have an i64 tag and one i64 payload slot; fieldless enums have only an i64 tag. Unused payloads are initialized to zero. These aggregate layouts are internal to generated code, not a portable C FFI layout.
- There is no early reclamation or tracing collection. Long-running allocation-heavy code retains memory until the region ends. Generic/nested arrays, string enum payloads, and capturing closures remain unsupported, rather than being silently represented unsafely.

Core string buffers, array buffers, object boxes, and ownership bookkeeping use checked sizing and fallible allocation. Overflow/failed reservation exits 1 with an allocation diagnostic. Standard I/O errors are also runtime errors instead of Rust panics. Capacity-overflow tests exercise failure handling deterministically without exhausting machine memory. Unicode case conversion maps scalar values (a scalar can expand to several scalars); locale/context-dependent casing is not provided.

This covers runtime-controlled reservations, not every allocation inside LLVM, the compiler, or host standard-library initialization. An OS-level out-of-memory kill after a successful virtual-memory reservation cannot be converted into a language diagnostic.

The experimental `nerv.collections` integer handles and raw-pointer/free exports are **not** part of the safe language core. Foreign callers must provide live, correctly typed, aligned handles, valid buffer lengths, and C-compatible callbacks; they must not free an iterated array during callbacks. Array map/reduce copy each element before calling native code, so callbacks may mutate shared storage without keeping overlapping Rust borrows alive. Forged handles and use-after-free remain invalid native operations.

## Shared entry and operations

[`crates/nerv-runtime/include/nerv.h`](../crates/nerv-runtime/include/nerv.h) specifies the core C ABI. `nerv_start_i64` / `nerv_start_void` enter a region, invoke a generated entry, print its result (zero for void), and clean up before returning process status 0. The same runtime implementation provides operations to the JIT's symbol registry and to a statically linked standalone program. Returning 7 prints `7`; it does not set process status 7.

Runtime checks include negative/oversized allocations, every array access, map/reduce lengths, integer division/remainder by zero, failed assertions, and invalid float-to-i64 conversions. Generated arithmetic wraps i64 add/subtract/multiply/negate and handles `MIN / -1` and `MIN % -1` without LLVM undefined behavior.

## Memory validation

```sh
rustup component add --toolchain nightly miri rust-src
bash ./tools/check-memory.sh
```

Miri checks the safe core under strict provenance, including shared handles, repeated region cleanup, parent-region explicit release, and reentrant callbacks. Miri checks for invalid access and leaks; subprocess-only fatal-exit tests run in the normal Rust suite instead. The experimental integer-handle collection test is excluded from strict-provenance checking; it is still covered by normal tests. Miri does not execute JIT machine code. JIT tests separately validate compound returns, nested structs, shared arrays/strings, arithmetic failures, and repeated invocation cleanup.

On macOS, a compiled example can also be checked with `leaks --atExit -- ./hello`. The local nested/shared-value example reports zero leaked bytes. This is a separate native-tool check, not part of the normal test percentage.
