# Semantic analysis and checked IR

`src/sema.nerv` implements semantic analysis in Nerv. It accepts a loaded source graph (or a single parsed module through the test convenience API) and returns either `CheckedProgram` or the first spanned `SemanticError`. Failed analysis never exposes partial IR. It does not call the Rust checker.

## Implemented

- Canonical local module loading, deduplication, import cycles/missing-file diagnostics, source IDs, qualified references, local-name preference, and ambiguity detection.
- Unconstrained generic function inference from arguments/immediate result annotations, concrete specialization, instance reuse, recursion, nested array/function signatures, and a 256-instance limit. Unused generic bodies are deferred.

- Interned scalar, array, function, and nominal types; distinct integer widths/signedness remain identified by their type names.
- Compilation-local symbol IDs for types, fields, variants, functions, parameters, locals, and captures.
- Declaration/signature collection before body checking, supporting forward calls and recursion.
- Lexical lookup, shadowing, scope restoration, immutable parameters/range variables, mutable locals, and assignment validation.
- Numeric operations/conversions, content string equality, array identity equality, homogeneous arrays, indexing, and repetition.
- Function arguments/results, direct and indirect calls, method receivers, field slots, and source-order struct initialization.
- Generic and non-generic structs/enums, concrete member signatures, payload construction, variant resolution, exhaustiveness, payload binding, and fieldless-enum equality.
- Result/Option propagation with concrete failure constructors and compatible error payloads.
- Expected-context and initializer-based generic constructor inference, instance reuse, and recursive layout checking.
- Detection of duplicate declarations/members/parameters, unknown types/names, and infinitely sized recursive nominal layouts.
- Conditions, loops, loop-exit context, early returns, and branch result compatibility.
- Typed lambdas, lifted function bodies, explicit capture identities and capture inputs, and nested captures.
- Runtime catalog for 119 fixed signatures plus print/assert, numeric casts, array length, and overloaded abs. Native ABI names are recorded; print arguments receive explicit width/float promotions.
- Main-signature validation, first-error byte spans, and `--check`/`--ir` driver modes.

## Still required for the full language

Semantic analysis is not complete. Project dependency manifests, generic function values without direct-call inference, and broader conformance remain unfinished. Trait objects and dynamic dispatch remain unsupported. Those unsupported forms still fail explicitly. The checker now analyzes its own 13-source import graph and all four bundled standard sources, but it does not generate machine code or build itself.

The stage-0 compiler rejects wildcard matching. Qualified enum match patterns are accepted here even where stage 0 rejects them; the module harness records that extension separately. Diagnostic wording is not fully compatible. Empty array literals default to []i64 without an expected element type, matching stage 0. Named void functions discard implicit final values; explicit return types and lambda results retain their checks.

There is no LLVM code generation, native C interoperability, or execution of checked IR yet. The approved backend direction is direct LLVM C API calls from Nerv. C interoperability will need bootstrap support in stage 0 before the self-hosted implementation can use it; a Rust backend bridge is not the selected architecture.

## Representation

`src/ir.nerv` defines `TypeTag`, `IROp`, `SemanticType`, `Symbol`, `CheckedNode`, `Capture`, and `CheckedProgram`. All runtime values in a successful program have a concrete type ID. Types 0–6 are void, bool, i64, f64, string, bottom, and the internal error sentinel. No successful symbol/operation may use the error sentinel. Bottom marks terminating control flow; it is not a value representation for LLVM.

Nominal types refer to declaration symbols. Function-type parts contain parameter types followed by the result. Array-type parts contain the element type. Symbols store category, type, owner, layout slot, mutability, source-module ID, and syntax provenance. Functions, locals, calls, fields, and constructors resolve to symbol IDs; backend consumers do not look up source names.

Each checked node stores operation, type ID, resolved symbol where applicable, slot metadata, decoded literal/operator text, original syntax ID, source-module ID, an exclusive byte span, and ordered child IDs. Syntax IDs are diagnostic provenance, not instructions to revisit syntax for type/name resolution. Children point to earlier checked nodes. Discarded provisional nodes may remain in the arena; function roots select the executable graph.

`Struct` initialization children retain source evaluation order and carry their resolved field slots. Loop slots distinguish inclusive/exclusive bounds. Closure expressions reference lifted function symbols and carry capture-value children. Capture records map the lifted function and original binding to its capture symbol and slot. Function roots include lifted lambdas.

Assignments carry their resolved mutable root symbol and checked target/value nodes. The target node describes field/index addressing; a future backend must emit a store rather than accidentally evaluating it only as a load. Array bounds and arithmetic guards remain runtime/code-generation work.

## Driver output

`--check FILE` reports success or a location/message with status 1. `--ir FILE` emits JSON arrays, one per line:

```text
["source", module_id, canonical_path]
["type", id, tag, name, nominal_symbol, part_type_ids]
["symbol", id, name, category, type_id, owner_symbol, slot, mutable, module_id]
["node", id, operation, type_id, symbol_id, slot, start, end, text, child_ids, module_id]
["capture", function_symbol, original_symbol, capture_symbol, slot]
["program", main_symbol_or_minus_one, function_root_ids]
```

This is a development inspection format, not a stable public interchange ABI. Symbols/nodes with no applicable owner or slot use `-1`. A library module may have no main.

## Verification

The semantic suite has 11 direct tests. The differential harness compares 24 accepted and 41 rejected cases with Nerv 1.0.0 and validates 252 checked operations, type references, symbols, capture mappings, function roots, and UTF-8 spans. Unsupported-feature tests verify explicit failures and no partial output. Lexer/parser regression suites remain part of validation. Verification so far is Windows-native; macOS/Linux and eventual runtime code generation need their own checks.

Next: add project path dependencies and direct LLVM integration after C interoperability. Preserve concrete identities and source provenance. Keep incomplete features rejected until their typed IR and diagnostics are verified.

## Module and generic-function stage

`src/loader.nerv` parses source files into one arena with an independent node-to-source table. `SourceModule` retains text, canonical path, declared module name, and import edges. Module 0 is the selected entry source. Imported modules can have their own functions named main without becoming the program entry. Load errors and semantic errors retain source provenance.

Generic function templates are retained separately from concrete symbols. Direct calls infer type arguments from parameter patterns and immediate result context. Concrete signatures are registered once per template/type-argument tuple, and a work queue checks their bodies with the template's module and a fresh type environment. Recursive calls reuse the registered instance. The returned IR contains concrete functions and types, never unresolved type parameters or unchecked template bodies. Trait bounds resolve in the defining module and require a concrete nominal implementation claim. Generic nominal instances share the concrete-specialization budget with functions.

Verification adds six direct generic tests, 15 stage-0 differential generic-function cases, and 21 multi-file compatibility cases plus the separately identified qualified-pattern extension. These include nested captures, recursive specialization, an expanding-recursion limit, conflicting inference, duplicate names across modules, lexical shadowing, diamond imports, and diagnostics in dependencies. Project manifests, case-insensitive filesystem edge cases beyond canonicalization, and native non-Windows validation remain outside this coverage.

## Generic nominal types and propagation

Generic struct/enum templates are instantiated by concrete type-argument tuples. An instance receives a distinct nominal symbol before fields/payloads are resolved so recursive references can reuse it. Concrete members are registered under that owner. Method bodies restore the owner's parameter environment, rather than using an overwritten syntax-to-symbol entry from another instance. Type parts retain concrete arguments; backend layout uses owner symbols and field/variant metadata.

Constructor inference uses expected type context and field/payload types. Initializer evaluation order remains the source order, with resolved field slots. Result/Option propagation uses the actual success-variant payload type and compares actual failure payload types, rather than assuming a user-defined container's parameter order dictates its layout. Specialized target failure constructors are explicit in Try IR.

Functions and nominal types share a 256-specialization budget. Active nominal instantiation depth is limited to 64; cache hits on recursive references do not increase that depth. Recursive nominal layouts through array handles are allowed; infinitely sized by-value cycles are rejected.

## Runtime catalog and verification

`src/runtime.nerv` contains signatures and native ABI mapping derived from the Rust compiler's `runtime_type` and `runtime_symbol` implementations. These cover text, paths, structured file operations, process/time I/O, JSON, network clients, tasks/allocation scopes, numeric/string/array operations, and legacy collections. Unknown names or wrong signatures are rejected. Numeric casts and assertions remain intrinsic operations; print arguments are promoted explicitly to their native ABI widths.

Final Windows verification passed 35 unit tests; 35 generic-type/propagation differential cases; 134 runtime cases covering all 119 fixed signatures, native export names, promotions, and invalid arguments; 26 positive/41 negative core semantic cases; 15 generic-function cases; and 23 multi-file compatibility cases plus the tracked qualified-pattern extension. Own-source IR validation covered 13 sources, 396 functions, and 23,565 checked operations. Each of the four bundled standard-source modules passed the same IR validation. Parser/lexer regressions also passed. This does not prove exhaustive language compatibility or executable LLVM lowering.

## Static trait stage

The checker now supports trait declarations, inline claims, explicit and inherent implementation blocks, method-owner attachment, and bounds on generic functions/nominal types. Claims are validated against concrete signatures with receiver substitution, including specialized generic owners. Trait/module identities are separate; a same-named trait in another module does not satisfy a bound. All emitted calls remain concrete, and compile-time trait metadata is not emitted as a runtime trait type.

The checker rejects missing or mismatched claimed methods even if unused, where stage 0 accepts those programs. These stricter contract checks have separate tests. Unused trait signatures are not exhaustively checked without a concrete claim, matching the current staged approach. See TRAITS.md for syntax and scope.

Verification passed 40 unit tests; 15 trait compatibility cases plus two stricter contract cases; 25 multi-file cases plus the qualified-pattern extension; and all earlier nominal, runtime, semantic, and generic suites. Own-source IR validation passed on 13 sources, 412 functions, and 24,638 checked operations, plus all bundled standard modules. Updated parser validation covered 109 cases and 21,067 expression shapes/spans across 93 accepted modules; lexer validation covered 42 source files and 59 generated/focused cases. Windows-native verification only; no generated LLVM code was executed.

## Project manifest and lock stage

Implemented `manifest.nerv`, `project.nerv`, and `sha256.nerv`, integrated with the loader and driver. Project aliases resolve in the importing package, canonical roots deduplicate packages, cycles/missing libraries are rejected, and deterministic SHA-256 manifest locks match stage 0 in the Windows fixtures. The driver accepts `--manifest-path`, `--locked`, and omitted source paths with entry/library selection. See [PROJECTS.md](PROJECTS.md) for syntax, limits, and differences.

The project suite covers 29 cases and validates resolved IR, exact locks, missing/stale locks, source-only edits, refreshes, and explicit manifests from another working directory. The checker now validates its own graph of 16 sources with 512 function roots and 30,368 checked operations. All four bundled standard-source modules and the existing semantic/trait/generic/module/runtime suites pass. Project checking covers the selected entry/import graph; whole-project scanning and test execution remain future driver work. Native non-Windows validation is outstanding.

Next: bootstrap C interoperability and direct LLVM C API bindings/lowering, followed by object generation/linking and stage-1/stage-2 self-compilation. Earlier pending-manifest statements above describe historical milestones. `Nerv-v2` remains unchanged.

Final project-stage verification also passed all 48 unit tests (21 test/source files), formatting, locked stage-0 checking, native release building, and Python harness syntax checks.
