# Native interoperability and LLVM

The compiler's bindings in `src/llvm.nerv` call LLVM's C API through native function addresses. `src/backend.nerv` lowers resolved IR, builds concrete types/functions/closures, verifies the module, applies LLVM's O2 pipeline for release builds, and emits native objects. `src/linker.nerv` runs the configured linker. No Rust compiler/backend process is used after the initial bootstrap.

## Native calls

The low-level `nerv.ffi` namespace supports dynamically loading a library (`open`), looking up a symbol (`symbol`), and closing the library (`close`). An unsuccessful lookup/load returns zero. Calls use `call_i8`, `call_u8`, `call_i16`, `call_u16`, `call_i32`, `call_u32`, `call_i64`, `call_u64`, `call_isize`, `call_usize`, `call_f32`, `call_f64`, or `call_void`. The suffix selects the native return type. The first argument is the symbol address; remaining numeric arguments retain their explicit widths.

```nerv
let library = nerv.ffi.open("example.dll")
assert(library != 0)
let function = nerv.ffi.symbol(library, "increment")
assert(function != 0)
let answer = nerv.ffi.call_i32(function, i32(41))
nerv.ffi.close(library)
```

Native pointers are represented as raw 64-bit addresses. Zero represents null. The runtime rejects building this interface for a non-64-bit target. These are low-level operations: the caller must supply a valid address, the exact fixed C signature, live arguments, and the matching ownership protocol. C variadic functions, native aggregate-by-value signatures, and raw C callback adapters are not exposed by this interface. LLVM bindings used by this compiler require none of those facilities.

`cstring` allocates NUL-terminated UTF-8 and rejects embedded NUL; `bytes` allocates raw UTF-8 bytes with a trailing zero. `text` copies a borrowed NUL-terminated string into a managed Nerv string. `alloc` creates zeroed, eight-byte-aligned native storage. `load`/`store` access an unaligned 64-bit word; `load_byte`/`store_byte` access one byte. `free` accepts only allocations returned by `alloc`, `cstring`, or `bytes`. LLVM-owned handles/messages use their LLVM disposal functions instead.

`run` launches an executable with an argument array and inherits standard streams/environment. It does not invoke a shell. `executable` returns the current executable path for asset discovery. `hash` and `file_hash` provide native SHA-256 for build caching; an unreadable file produces an empty digest and cannot yield a cache hit. `copy`, `lock`, and `unlock` support output publication and serialized builds. Manifest hashing also has its independent Nerv implementation in `src/sha256.nerv`.

## Backend representation

Managed strings, arrays, and closures use opaque runtime handles. Array indexing accesses the documented data/length prefix and checks bounds before forming an element address. Concrete structs use named LLVM struct types. Payload enums use a tag plus one typed slot per variant; payloadless enums use an integer tag. Generic types and methods have already been specialized by semantic analysis.

Closures contain a code pointer and heap-backed environment. Captures are stored by value, retaining managed array/function handles. Thunks adapt ordinary functions to the closure convention. Mutable locals use function-entry allocations so loops do not accumulate stack allocations. Control flow retains early returns and loop exits; boolean operators short-circuit. Integer division handles zero and signed minimum divided by minus one. Floating-to-integer conversions validate range and NaN before conversion.

LLVM contexts, modules, builders, target data, target machines, pass options, error messages, and temporary native buffers have explicit disposal. The managed compiler allocation region lasts for one process. Project tests run each source file in a child compiler process to release that region between compilations.

`tools/generate-llvm.py` maintains the binding list; after regenerating, run the Nerv formatter. The bindings follow the LLVM 23 C headers used by the isolated bootstrap. The shared library must provide those symbols.

## Validation

`tools/check-codegen.py` runs executable cases in debug and release against stage 0. `tools/check-ffi.py` builds a small native test library and checks integer widths, floating returns/arguments, mixed registers, stack arguments, void calls, pointer writes, and ownership. `tools/check-driver.py` checks project creation, argument/exit forwarding, cache reuse/invalidation/corruption, forced rebuilds, concurrent builds, and preservation of an executable after a failed link. The bootstrap script runs these alongside the frontend, formatting, unit, and generation-comparison checks.
