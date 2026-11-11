# Self-hosting Nerv

Build the replacement compiler in this folder without changing the Rust compiler in `../Nerv-v2`. The released compiler is stage 0, the trusted starting executable. LLVM and the existing native runtime can remain native dependencies while the compiler moves to Nerv.

## Milestones

1. Nerv lexer: tokens, UTF-8 byte spans, locations, indentation, literals, and a `--lex` driver. Differential verification against stage 0 on real sources and focused cases.
2. Parser: arena-backed nodes and integer IDs, source spans, declaration/expression/statement parsing, and syntax diagnostics. Compare against the existing parser.
3. Semantic analysis: module loading, symbol/type arenas, resolution, type checking, generic specialization, closure lifting, and checked IR. Preserve the existing IR invariants and rejection tests.
4. Direct LLVM C API: implement C interoperability, opaque native handles, native-library linking, and explicit cleanup, then LLVM bindings in Nerv. Lower checked IR to LLVM, verify it, optimize, and emit objects directly. Keep the existing runtime prebuilt initially; do not introduce a Rust backend bridge.
5. Build driver: manifests, local dependencies, locks, native linking, test processes, formatting, and caches. Add process/binary I/O facilities when actually needed.
6. Bootstrap: stage 0 builds stage 1; stage 1 builds stage 2; stage 2 builds stage 3. Run the same conformance tests with each. Compare deterministic outputs after accounting for paths and metadata.
7. Distribution: exercise the self-built compiler and installers on Windows, macOS, and Linux before replacing the released Rust compiler.

## Design

- Keep UTF-8 byte spans and one-based Unicode-scalar columns compatible with stage 0, including its current LF-based location calculation.
- Represent recursive syntax and type graphs using arenas and IDs, not recursive inline structs.
- Convert source into scalar and offset tables once. Avoid repeatedly indexing the original string by character position.
- Use appendable buffers for tokens and arena storage. Keep all compiler allocations in a compilation region until a later lifetime design is verified.
- Keep token kind names compatible with stage 0 for differential output. This first token representation uses strings; parser node tags and symbol IDs will be explicit compiler data types.
- Return structured lexical errors; do not mistake lexical acceptance for type checking or compilation.
- Preserve the Rust frontend as the oracle while each replacement stage is developed.

## Current scope

The lexer, arena-based parser, local module loader, and initial semantic stage with generic function specialization are implemented in Nerv, with native command-line drivers and tests. Full semantic analysis, C interoperability/direct LLVM integration, and self-compilation remain unfinished. The project must not claim to compile programs or build itself until those stages pass verification.

## Initial milestone verification

On Windows with the published Nerv 1.0.0 compiler, formatting, locked project checking, four lexer unit tests, and the native release build passed. Differential checks matched stage 0 on 31 real source files and 59 focused/generated cases. Successful cases also passed independent byte-span, scalar-column, and token-text reconstruction checks. Rejected cases compare the first error location; complete malformed-literal error spans and continued error recovery are not yet equivalent. Native macOS/Linux verification remains outstanding.

## Parser milestone

The parser stores typed-tag syntax nodes in an appendable arena with integer child IDs and UTF-8 spans. `--parse` emits a JSON-lines arena dump. No recursive by-value syntax structs or Rust parser calls are used. Syntax nesting is bounded at 128 active parser calls; errors return no partial AST. Literal nodes retain source spelling for later semantic lowering.

On Windows, the release build, formatting, locked checking, and all 13 lexer/parser unit tests pass. Differential checks match syntax acceptance on 101 files/cases and 8,384 expression shapes/spans across 85 accepted modules. Declaration/type shapes, errors, and nesting limits have direct tests. The lexer regression suite also matches stage 0 on 34 source files and 59 focused/generated cases. These checks do not establish complete AST/diagnostic equivalence or native macOS/Linux compatibility.

The next milestone is semantic analysis. Establish symbol and type arenas, load modules explicitly, resolve qualified declarations, and elaborate types into checked IR while retaining source spans. Object emission will use LLVM's C API directly after C interoperability exists. Do not use stage 0 as a hidden compile fallback.

## Backend decision

The user selected direct LLVM C API calls from Nerv. A Rust backend bridge is no longer the planned production architecture. C interoperability must define native pointer/null handling, C calling conventions and numeric widths, string/buffer marshalling, library linking, callbacks where needed, and ownership/disposal of LLVM handles. This does not require porting LLVM or the linker to Nerv. No LLVM binding is claimed implemented by the semantic-analysis milestone.

## Semantic stage in progress

See [SEMANTICS.md](SEMANTICS.md). The first pass has concrete type interning, symbol IDs, declaration collection, scopes, type checking, monomorphic structs/enums/methods, control flow, captures, and typed IR. `--check` and `--ir` are available for this subset. Imports, generics, traits, the full runtime catalog, and generic propagation still fail explicitly.

Windows verification passed all 24 unit tests, native release building, and formatting/locked checks. Semantic acceptance matches stage 0 on 24 positive and 41 negative cases; 252 checked operations passed structural validation. Updated parser regression checks match 104 files/cases and 14,236 expression shapes/spans across 88 accepted modules. Lexer checks match 37 source files plus 59 focused/generated cases. These are bounded regression suites, not full-language compatibility or self-hosting proof.

Continue with module loading and specialization before implementing direct LLVM lowering. C interoperability needs initial bootstrap support in stage 0 as well as the Nerv implementation.

## Local modules and generic functions

Implemented canonical local source loading, duplicate-file/cycle detection, module namespaces, qualified function/type/variant resolution, and source-aware checked IR/diagnostics. Generic functions now infer arguments from parameter and immediate result types, specialize into concrete symbols/bodies, reuse instances, and support recursion with a specialization limit. Generic structs/enums, trait bounds, generic function values, project dependency manifests, the full runtime catalog, and propagation remain outstanding.

Six additional unit tests and differential suites cover generic functions and multi-file compilation. The new `stdlib/` source snapshot is included under the MIT license, but loading it does not yet imply that its generic types can be checked. Direct LLVM C API remains the selected backend; no LLVM bridge or code generation is introduced here.

Latest Windows verification: all 30 unit tests, formatting, locked checking, and native release build passed. Semantic differential checks passed 24 positive/41 negative cases and validated 252 checked operations. Generic-function checks passed 15 cases, including specialization overflow. Module checks passed 21 compatibility cases plus a separately recorded qualified-pattern extension that stage 0 rejects. Parser checks passed 106 files/cases and 17,105 expression shapes/spans across 90 accepted modules. Lexer checks passed 39 source files plus 59 focused/generated cases. The standard-source snapshot matches the reference checkout byte for byte. `Nerv-v2` remains unchanged.

## Verified generic nominal and own-source semantic stage

Generic structs/enums, concrete member/method instances, constructor inference, Result/Option propagation, and the runtime signature catalog are now implemented and tested. The rebuilt checker semantically checks its own source graph and all four bundled standard modules. This is not self-compilation: the binary is still produced by stage 0, and no LLVM object emission exists.

Windows checks passed: 35 unit tests; 35 nominal/propagation cases; 134 runtime cases with all 119 fixed signatures and native ABI validation; 26 positive/41 negative semantic cases; 15 generic-function cases; 23 multi-file compatibility cases plus one known extension. Own-source checked IR passed invariants across 13 sources, 396 functions, and 23,565 operations. Parser checks passed 108 cases and 20,303 expression shapes/spans across 92 accepted modules; lexer checks passed 41 source files and 59 focused/generated cases. Formatting, locked checks, native release build, and Python syntax checks passed.

Next: trait/implementation/constraint semantics and project path-dependency manifests. Then bootstrap C interoperability and direct LLVM C API bindings/lowering, followed by actual bootstrap stages and native platform verification. Earlier sections' pending generic-nominal/runtime items describe historical milestones; this section is the latest status.

## Verified static trait stage

Implemented trait registry/namespace identity, inline and explicit implementation claims, inherent methods, concrete method conformance, and generic function/type constraints. Trait receiver signatures substitute the implementing nominal type. Methods resolve directly after specialization; no dynamic trait objects are introduced. Missing/incompatible claimed methods are rejected more strictly than stage 0, with separate regression cases.

Latest Windows verification passed 40 unit tests, formatting, locked checking, and release building; 15 trait compatibility cases and two stricter contract cases; 25 module compatibility cases plus the tracked qualified-pattern extension; and all nominal/runtime/core semantic/generic checks. Own-source typed IR passed on 13 sources, 412 functions, and 24,638 operations. Parser checks passed 109 cases and 21,067 expression shapes/spans; lexer checks passed 42 source files and 59 focused/generated cases. Python harness syntax checks passed.

Next is project manifest/path-dependency resolution, followed by bootstrap C interoperability and the direct LLVM C API backend. Earlier trait-pending sections describe historical milestones. Self-compilation remains unimplemented.

## Project manifest and lock stage

Implemented `manifest.nerv`, `project.nerv`, and `sha256.nerv`, integrated with the loader and driver. Project aliases resolve in the importing package, canonical roots deduplicate packages, cycles/missing libraries are rejected, and deterministic SHA-256 manifest locks match stage 0 in the Windows fixtures. The driver accepts `--manifest-path`, `--locked`, and omitted source paths with entry/library selection. See [PROJECTS.md](PROJECTS.md) for syntax, limits, and differences.

The project suite covers 29 cases and validates resolved IR, exact locks, missing/stale locks, source-only edits, refreshes, and explicit manifests from another working directory. The checker now validates its own graph of 16 sources with 512 function roots and 30,368 checked operations. All four bundled standard-source modules and the existing semantic/trait/generic/module/runtime suites pass. Project checking covers the selected entry/import graph; whole-project scanning and test execution remain future driver work. Native non-Windows validation is outstanding.

Next: bootstrap C interoperability and direct LLVM C API bindings/lowering, followed by object generation/linking and stage-1/stage-2 self-compilation. Earlier pending-manifest statements above describe historical milestones. `Nerv-v2` remains unchanged.

Final project-stage verification also passed all 48 unit tests (21 test/source files), formatting, locked stage-0 checking, native release building, and Python harness syntax checks.
