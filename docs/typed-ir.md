# Checked intermediate representation

The execution pipeline is:

```text
source files → spanned syntax → semantic checking/resolution → checked IR → LLVM → JIT or host object
```

`nerv-sema::check_path` returns a `Program` containing source files, loaded syntax modules, and `Program.ir`. Embedded callers with already-loaded modules can use `check_modules`. The JIT's `run_main`, `run_tests`, and `run_benches` APIs accept only `&nerv_sema::ir::CheckedProgram`. Syntax is a **test-only** dependency of the JIT crate.

`emit_object` consumes the same checked program without constructing a JIT. See [standalone compilation](standalone.md) for object emission, native linking, and runtime packaging.

## IR invariants

- A deterministic, compilation-local `SymbolId` identifies each function, lifted lambda, nominal type, variant, parameter, local binding, and struct field declaration. IDs survive shadowing; references never use source names. Source/debug names do not control linkage.
- Every expression and binding carries its checked type. Nominal types refer to IDs, not names. Statements also carry their result type. Function types retain parameter and result types, including inferred function-valued bindings.
- Calls distinguish direct function IDs from indirect function-valued expressions. Methods are resolved direct calls with an explicit receiver argument. Enum construction and match arms carry their resolved enum/variant IDs and tags. Field operations carry declaration-order indices; construction retains **source evaluation order** separately from those indices.
- Numeric casts are explicit identity, integer-to-float, or checked float-to-integer conversion nodes. Runtime overloads resolve to ABI symbols before code generation. Alpha width aliases already share their specified representation; trait annotations constrain concrete bindings without introducing dynamic dispatch or runtime layout conversions.
- Non-capturing lambdas are lifted to ordinary IR functions during semantic resolution, not synthesized by LLVM lowering.
- Source spans remain on declarations, bindings, expressions, statements, and match arms, including lifted lambda bodies.

Only semantic analysis constructs `CheckedProgram`; callers and backends receive immutable views. The compiler retains its existing diagnostic checking pass, then resolves checked syntax and records its types in IR. It does not yet use a single-pass type-inference/resolution algorithm. The JIT does not call the checker, walk syntax, reconstruct source names, search methods by LLVM layout, or rediscover conversions. It translates types to LLVM representations and emits instructions, runtime guards, and calls. LLVM module verification remains an internal correctness check.

## Validation and migration

Restrictions previously discovered only while lowering (unsupported enum payloads/arrays, unknown nominal types, and invalid main signatures) now fail during `--check`, with source spans. Resolution also rejects duplicate fields, invalid constructor qualifiers, void parameters/fields, and infinitely sized by-value recursive structs before they can reach LLVM. Recursive functions and function-pointer signatures are supported; recursive inline struct layouts are not.

Compiler symbols use ID-based mangled names separate from runtime ABI symbols. Distinct fieldless enums may share an LLVM representation, but method dispatch uses their resolved nominal identity. Inferred callbacks, branch-local callbacks, and returned function values use their stored signatures instead of backend slot-name heuristics.

The earlier 19 JIT tests are retained and now pass through semantic checking. Two old backend-only array mutation tests now explicitly declare their bindings `mut`, matching the existing language rules. Added IR tests inspect IDs, shadowing, types, resolutions, conversions, lifting, and spans. Execution regressions cover identical-layout enum methods, higher-order calls, field evaluation order, recursion, runtime symbol collisions, and test/benchmark entry groups. Existing output/exit-status and diagnostic snapshots continue to run unchanged.

Run all tests with `bash ./tools/test.sh` or `.\tools\test.ps1`; see [testing](testing.md).
