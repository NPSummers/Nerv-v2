# Execution-region memory and runtime ABI (version 2)

Each JIT invocation or standalone entry owns a thread-local execution region. A test/benchmark batch shares one region. Allocations survive lexical blocks and function returns; region teardown releases every still-owned allocation exactly once. Regions nest in stack order, cannot migrate between threads, and native frees remove allocations from the owning region, including a parent region. Fatal errors print `runtime error: ...` and exit 1 without unwinding through generated code; the OS reclaims memory on fatal exit.

## Representation and lifetime

- Integers use their declared LLVM width and signedness; `isize`/`usize` use the host pointer width. Floats use IEEE binary32 or binary64, and booleans use the target's scalar bool representation. Narrow numeric values are promoted explicitly before calls to scalar print exports.
- Strings and arrays are pointer-sized handles. Strings own immutable UTF-8, arrays own fixed-length typed elements. Generated code never reads Rust `String`/`Vec` layouts. ABI v2 exposes an immutable C-layout array prefix (`int64_t *data; int64_t length;`) for inline checked accesses. Only elements are mutable; header loads, never element loads, are marked invariant. Backing storage cannot move or resize before destruction. String producers allocate distinct handles, including empty strings.
- Structs are target-native LLVM aggregates in declaration field order. Nested structs copy by value; string/array fields copy handles and share the allocation. Payload enums have an i64 tag followed by one typed slot per declared variant (an i64 placeholder for fieldless variants); fieldless enums have only an i64 tag. Unused slots are initialized to zero. Inline recursive aggregates are rejected. These aggregate layouts are internal to generated code, not a portable C FFI layout.
- There is no early reclamation or tracing collection. Long-running allocation-heavy code retains memory until the region ends. Typed arrays store inline elements in region-owned, eight-byte-aligned storage; nested arrays and strings store handles. Closures capture referenced outer values into a region-owned environment. String/array captures share their managed handles. Noncapturing function values use static descriptors.

Core string buffers, array buffers, object boxes, and ownership bookkeeping use checked sizing and fallible allocation. Overflow/failed reservation exits 1 with an allocation diagnostic. Legacy scalar output failures are fatal runtime errors. Standard-library file and stdin operations return ordinary `Result` values with `IoError` context instead of panicking. Capacity-overflow tests exercise failure handling deterministically without exhausting machine memory. Unicode case conversion maps scalar values (a scalar can expand to several scalars); locale/context-dependent casing is not provided.

This covers runtime-controlled reservations, not every allocation inside LLVM, the compiler, JSON/TLS/networking dependencies, or host standard-library initialization. An OS-level out-of-memory kill after a successful virtual-memory reservation cannot be converted into a language diagnostic.

The experimental `nerv.collections` integer handles and raw-pointer/free exports are **not** part of the safe language core. Foreign callers must provide live, correctly typed, aligned handles, valid buffer lengths, and C-compatible callbacks; they must not free an iterated array during callbacks. Array map/reduce copy each element before calling native code, so callbacks may mutate shared storage without keeping overlapping Rust borrows alive. Forged handles and use-after-free remain invalid native operations.

## Shared entry and operations

Generated standalone entries reference `nerv_runtime_abi_v2`; an older runtime fails to link rather than misreading the array layout. Inline accesses reject null handles and use unsigned index/length comparisons, rejecting negative indices as well as oversized indices before computing element addresses. Native callers must not modify the header.

[`crates/nerv-runtime/include/nerv.h`](../crates/nerv-runtime/include/nerv.h) specifies the core C ABI. `nerv_start_i64` / `nerv_start_void` enter a region, invoke a generated entry, print its result (zero for void), and clean up before returning process status 0. The same runtime implementation provides operations to the JIT's symbol registry and to a statically linked standalone program. Returning 7 prints `7`; it does not set process status 7.

Runtime checks include negative/oversized allocations, every array access, map/reduce lengths, integer division/remainder by zero, failed assertions, and invalid float-to-i64 conversions. Generated arithmetic wraps i64 add/subtract/multiply/negate and handles `MIN / -1` and `MIN % -1` without LLVM undefined behavior.

## Memory validation

```sh
rustup component add --toolchain nightly miri rust-src
bash ./tools/check-memory.sh
```

Miri checks the safe core under strict provenance, including shared handles, repeated region cleanup, parent-region explicit release, and reentrant callbacks. Miri checks for invalid access and leaks; subprocess-only fatal-exit tests run in the normal Rust suite instead. The experimental integer-handle collection test is excluded from strict-provenance checking; it is still covered by normal tests. Miri does not execute JIT machine code. JIT tests separately validate compound returns, nested structs, shared arrays/strings, arithmetic failures, and repeated invocation cleanup.

On macOS, a compiled example can also be checked with `leaks --atExit -- ./hello`. The local nested/shared-value example reports zero leaked bytes. This is a separate native-tool check, not part of the normal test percentage.

Project process-entry adapters use the same execution region but return the integer main result modulo 256 as an exit status; void returns zero. They do not print an implicit result. Historical standalone/JIT adapters retain result printing.

JSON primitives exchange serialized UTF-8 text and error results; the source `Json` wrapper is an ordinary Nerv struct. HTTP clients and WebSocket drivers are native runtime services. WebSocket IDs are validated registry keys, not pointer casts. Each event callback receives a nested execution region; its returned state is copied before the region is destroyed. The socket and owned state remain in the native driver, and callbacks must not retain their argument pointers through foreign embedding APIs. Ordinary outer-region allocations still retain their original lifetime semantics. See [networking](networking.md).

## Closure callbacks, allocation scopes, and tasks

Function values are pointers to C-layout closure descriptors containing `code` and `environment` pointers followed by an `i64` mutable-capture flag. Compiled indirect calls invoke a thunk with the environment as its first argument, followed by the declared parameters. Captured values occupy a typed aggregate stored in a managed array. Ordinary direct function calls keep their original signatures. The original foreign array-map/reduce and WebSocket callback exports remain available; newly generated callbacks use distinct `_closure` exports. A runtime without these new exports cannot link a new application requiring them.

Noncapturing descriptors are immutable LLVM globals with null environments. Capturing descriptors and environments belong to the current execution region. The WebSocket adapter rejects captured arrays, aggregates, and function values to prevent event-region handles escaping through shared mutable storage. It maintains a scoped callback stack on its invoking thread, supporting nested callback runs without retaining event arguments after return.

`nerv_memory_scope` requires a noncapturing `fn(i64) -> i64` closure and wraps its invocation in an `Execution` region. The integer result is returned after cleanup. Managed handles cannot escape through the typed result. `nerv_task_spawn` enforces the same callback restriction, copies its function pointer, and invokes it on a native thread with its own `Execution`. Only integer input/results and an atomic cancellation flag cross threads. Parent region teardown cancels and joins its outstanding task handles before reclaiming allocations. Cancellation is cooperative.

The existing unsafe legacy integer/pointer APIs are outside these guarantees. Native callers must uphold callback signatures and handle lifetimes.
